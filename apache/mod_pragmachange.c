/* Apache owns request lifetimes; Rust owns immutable models. Shared history
 * contains offsets and serialized bytes only, never worker-heap pointers. */
#include "httpd.h"
#include "http_config.h"
#include "http_connection.h"
#include "http_log.h"
#include "http_protocol.h"
#include "http_request.h"
#include "ap_mpm.h"
#include "unixd.h"
#include "apr_global_mutex.h"
#include "apr_shm.h"
#include "apr_rmm.h"
#include "apr_strings.h"
#include "util_filter.h"
#include "pragmachange.h"
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#ifndef PC_MODULE_VERSION
#define PC_MODULE_VERSION "0.1.0"
#endif
#define PC_BUCKETS 4093
#define PC_KEY_BYTES 101
#define PC_MAX_FILE (64 * 1024 * 1024)

module AP_MODULE_DECLARE_DATA pragmachange_module;
typedef apr_rmm_off_t pc_off;
typedef struct {
  pc_off next, prev, hash_next, data;
  size_t len;
  unsigned char key[PC_KEY_BYTES];
} pc_entry;
typedef struct {
  pc_off buckets[PC_BUCKETS], first, last;
  size_t count;
} pc_shared;
typedef struct {
  apr_shm_t *shm;
  apr_rmm_t *rmm;
  apr_global_mutex_t *mutex;
  const char *lock_path;
  pc_shared *shared;
  size_t size;
} pc_store;
typedef struct {
  const char *release_path, *secret_path, *guard_name;
  size_t size;
  int32_t guard_index;
  size_t guard_max_body;
  PcRuntime *runtime;
  pc_store *store;
} pc_server;
typedef struct { int enabled; } pc_location;
typedef struct {
  unsigned char *body;
  size_t length, position;
  int complete, inspected;
} pc_request;
static int pc_enabled_seen;

static PcSlice pc_string(const char *s) {
  PcSlice result = {(const uint8_t *)s, s ? strlen(s) : 0};
  return result;
}
static pc_entry *pc_at(pc_store *s, pc_off off) {
  return off ? apr_rmm_addr_get(s->rmm, off) : NULL;
}
static unsigned pc_hash(PcSlice key) {
  uint32_t h = 2166136261u;
  size_t i;
  for (i = 0; i < key.len; ++i) h = (h ^ key.data[i]) * 16777619u;
  return h % PC_BUCKETS;
}
static pc_off pc_find(pc_store *s, PcSlice key) {
  pc_off off;
  for (off = s->shared->buckets[pc_hash(key)]; off; off = pc_at(s, off)->hash_next)
    if (!memcmp(pc_at(s, off)->key, key.data, PC_KEY_BYTES)) return off;
  return 0;
}
static void pc_unlink_lru(pc_store *s, pc_off off) {
  pc_entry *e = pc_at(s, off);
  if (e->prev) pc_at(s, e->prev)->next = e->next;
  else s->shared->first = e->next;
  if (e->next) pc_at(s, e->next)->prev = e->prev;
  else s->shared->last = e->prev;
}
static void pc_touch(pc_store *s, pc_off off) {
  pc_entry *e = pc_at(s, off);
  e->prev = 0;
  e->next = s->shared->first;
  if (e->next) pc_at(s, e->next)->prev = off;
  else s->shared->last = off;
  s->shared->first = off;
}
static void pc_remove(pc_store *s, pc_off off) {
  pc_entry *e = pc_at(s, off);
  PcSlice key = {e->key, PC_KEY_BYTES};
  pc_off *link = &s->shared->buckets[pc_hash(key)];
  while (*link != off) link = &pc_at(s, *link)->hash_next;
  *link = e->hash_next;
  pc_unlink_lru(s, off);
  apr_rmm_free(s->rmm, e->data);
  apr_rmm_free(s->rmm, off);
  --s->shared->count;
}
static int pc_evict(pc_store *s, pc_off keep, const uint8_t *ns) {
  pc_off off;
  for (off = s->shared->last; off; off = pc_at(s, off)->prev) {
    if (off != keep && (!ns || !memcmp(pc_at(s, off)->key, ns, 36))) {
      pc_remove(s, off);
      return 1;
    }
  }
  return 0;
}
static int32_t pc_get(void *ctx, PcSlice key, PcSlice *value) {
  pc_store *s = ctx;
  pc_off off;
  pc_entry *e;
  if (key.len != PC_KEY_BYTES) return -1;
  off = pc_find(s, key);
  if (!off) return 0;
  e = pc_at(s, off);
  value->data = apr_rmm_addr_get(s->rmm, e->data);
  value->len = e->len;
  return 1;
}
static int32_t pc_put(void *ctx, PcSlice key, PcSlice value, size_t maximum) {
  pc_store *s = ctx;
  pc_off off, data, cursor;
  pc_entry *e;
  size_t count = 0;
  if (key.len != PC_KEY_BYTES || !maximum || !value.len || value.len > s->size) return -1;
  off = pc_find(s, key);
  if (!off) {
    for (cursor = s->shared->first; cursor; cursor = pc_at(s, cursor)->next)
      if (!memcmp(pc_at(s, cursor)->key, key.data, 36)) ++count;
    if (count >= maximum && !pc_evict(s, 0, key.data)) return -1;
  }
  data = apr_rmm_malloc(s->rmm, value.len);
  while (!data && pc_evict(s, off, NULL)) data = apr_rmm_malloc(s->rmm, value.len);
  if (!data) return -1;
  memcpy(apr_rmm_addr_get(s->rmm, data), value.data, value.len);
  if (off) {
    e = pc_at(s, off);
    pc_unlink_lru(s, off);
    apr_rmm_free(s->rmm, e->data);
  } else {
    off = apr_rmm_calloc(s->rmm, sizeof(*e));
    while (!off && pc_evict(s, 0, NULL)) off = apr_rmm_calloc(s->rmm, sizeof(*e));
    if (!off) { apr_rmm_free(s->rmm, data); return -1; }
    e = pc_at(s, off);
    memcpy(e->key, key.data, PC_KEY_BYTES);
    e->hash_next = s->shared->buckets[pc_hash(key)];
    s->shared->buckets[pc_hash(key)] = off;
    ++s->shared->count;
  }
  e->data = data;
  e->len = value.len;
  pc_touch(s, off);
  return 0;
}
static void *pc_create_server(apr_pool_t *p, server_rec *s) {
  pc_server *c = apr_pcalloc(p, sizeof(*c));
  (void)s;
  c->size = 64 * 1024 * 1024;
  c->guard_index = -1;
  return c;
}
static void *pc_create_location(apr_pool_t *p, char *path) {
  pc_location *c = apr_pcalloc(p, sizeof(*c));
  (void)path;
  c->enabled = -1;
  return c;
}
static void *pc_merge_location(apr_pool_t *p, void *base, void *over) {
  pc_location *a = base, *b = over, *c = apr_pcalloc(p, sizeof(*c));
  c->enabled = b->enabled == -1 ? a->enabled : b->enabled;
  return c;
}
static const char *pc_path(cmd_parms *cmd, void *unused, const char *path) {
  pc_server *c = ap_get_module_config(cmd->server->module_config, &pragmachange_module);
  const char *error = ap_check_cmd_context(cmd, GLOBAL_ONLY);
  (void)unused;
  if (error) return error;
  path = ap_server_root_relative(cmd->pool, path);
  if (!path) return "PragmaChange requires an absolute or ServerRoot-relative path";
  if (!strcmp(cmd->cmd->name, "PragmaChangeRelease")) c->release_path = path;
  else c->secret_path = path;
  return NULL;
}
static const char *pc_size(cmd_parms *cmd, void *unused, const char *arg) {
  pc_server *c = ap_get_module_config(cmd->server->module_config, &pragmachange_module);
  char *end;
  unsigned long n;
  const char *error = ap_check_cmd_context(cmd, GLOBAL_ONLY);
  (void)unused;
  if (error) return error;
  errno = 0;
  n = strtoul(arg, &end, 10);
  if (errno || *end || n < 1 || n > 4096) return "PragmaChangeStateMiB must be 1..4096";
  c->size = (size_t)n * 1024 * 1024;
  return NULL;
}
static const char *pc_guard_name(cmd_parms *cmd, void *unused, const char *name) {
  pc_server *c = ap_get_module_config(cmd->server->module_config, &pragmachange_module);
  const char *error = ap_check_cmd_context(cmd, GLOBAL_ONLY);
  (void)unused;
  if (error) return error;
  if (!*name) return "PragmaChangeGuard requires a policy name";
  c->guard_name = apr_pstrdup(cmd->pool, name);
  return NULL;
}
static const char *pc_enable(cmd_parms *cmd, void *config, int enabled) {
  pc_location *c = config;
  const char *error = ap_check_cmd_context(cmd, NOT_IN_HTACCESS);
  if (error) return error;
  c->enabled = enabled;
  if (enabled) pc_enabled_seen = 1;
  return NULL;
}
static const command_rec pc_commands[] = {
  AP_INIT_TAKE1("PragmaChangeRelease", pc_path, NULL, RSRC_CONF, "Model release JSON path"),
  AP_INIT_TAKE1("PragmaChangeSecretFile", pc_path, NULL, RSRC_CONF, "Local 64-hex secret path"),
  AP_INIT_TAKE1("PragmaChangeStateMiB", pc_size, NULL, RSRC_CONF, "Shared history capacity in MiB (default 64)"),
  AP_INIT_TAKE1("PragmaChangeGuard", pc_guard_name, NULL, RSRC_CONF, "Release ingress policy applied to every enabled request"),
  AP_INIT_FLAG("PragmaChange", pc_enable, NULL, ACCESS_CONF, "Enable request inspection in this location"),
  {NULL}
};
static apr_status_t pc_free_runtime(void *p) { pc_runtime_free(p); return APR_SUCCESS; }
static unsigned char *pc_read(apr_pool_t *p, const char *path, size_t limit, size_t *len) {
  FILE *f = fopen(path, "rb");
  unsigned char *data;
  long length;
  if (!f) return NULL;
  if (fseek(f, 0, SEEK_END) || (length = ftell(f)) < 0 || (size_t)length > limit || fseek(f, 0, SEEK_SET)) {
    fclose(f); return NULL;
  }
  data = apr_palloc(p, (size_t)length + 1);
  *len = fread(data, 1, (size_t)length, f);
  if (*len != (size_t)length || ferror(f)) { fclose(f); return NULL; }
  fclose(f); data[*len] = 0;
  return data;
}
static int pc_hex(unsigned char c) {
  if (c >= '0' && c <= '9') return c - '0';
  if (c >= 'a' && c <= 'f') return c - 'a' + 10;
  if (c >= 'A' && c <= 'F') return c - 'A' + 10;
  return -1;
}
static int pc_pre_config(apr_pool_t *a, apr_pool_t *b, apr_pool_t *c) {
  (void)a; (void)b; (void)c; pc_enabled_seen = 0; return OK;
}
static int pc_check_config(apr_pool_t *p, apr_pool_t *log, apr_pool_t *temp, server_rec *s) {
  pc_server *c = ap_get_module_config(s->module_config, &pragmachange_module);
  unsigned char *release, *hex, secret[32], error[256] = {0};
  size_t length, hex_length, i;
  (void)log;
  if (!c->release_path && !c->secret_path && !pc_enabled_seen) return OK;
  if (!c->release_path || !c->secret_path) goto bad_config;
  release = pc_read(temp, c->release_path, PC_MAX_FILE, &length);
  hex = pc_read(temp, c->secret_path, 66, &hex_length);
  if (!release || !hex) goto bad_config;
  while (hex_length && (hex[hex_length-1] == '\n' || hex[hex_length-1] == '\r')) --hex_length;
  if (hex_length != 64) goto bad_config;
  for (i = 0; i < 32; ++i) {
    int hi = pc_hex(hex[i*2]), lo = pc_hex(hex[i*2+1]);
    if (hi < 0 || lo < 0) goto bad_config;
    secret[i] = (unsigned char)((hi << 4) | lo);
  }
  c->runtime = pc_runtime_new((PcSlice){release, length}, (PcSlice){secret, 32}, error);
  memset(secret, 0, sizeof(secret)); memset(hex, 0, 64);
  if (!c->runtime) {
    ap_log_error(APLOG_MARK, APLOG_ERR, 0, s, "PragmaChange: invalid release: %s", error);
    return HTTP_INTERNAL_SERVER_ERROR;
  }
  if (pc_ingress_policy_count(c->runtime) != 0) {
    if (!c->guard_name) {
      ap_log_error(APLOG_MARK, APLOG_ERR, 0, s,
                   "PragmaChange: a guarded release requires PragmaChangeGuard");
      pc_runtime_free(c->runtime);
      c->runtime = NULL;
      return HTTP_INTERNAL_SERVER_ERROR;
    }
    c->guard_index = pc_guard_policy(c->runtime, pc_string(c->guard_name), &c->guard_max_body);
    if (c->guard_index < 0 || c->guard_max_body >= PC_MAX_FILE) {
      ap_log_error(APLOG_MARK, APLOG_ERR, 0, s,
                   "PragmaChange: configured guard policy is missing or exceeds the Apache body limit");
      pc_runtime_free(c->runtime);
      c->runtime = NULL;
      return HTTP_INTERNAL_SERVER_ERROR;
    }
  } else if (c->guard_name) {
    ap_log_error(APLOG_MARK, APLOG_ERR, 0, s,
                 "PragmaChange: configured guard policy is absent from release");
    pc_runtime_free(c->runtime);
    c->runtime = NULL;
    return HTTP_INTERNAL_SERVER_ERROR;
  }
  apr_pool_cleanup_register(p, c->runtime, pc_free_runtime, apr_pool_cleanup_null);
  return OK;
bad_config:
  ap_log_error(APLOG_MARK, APLOG_ERR, 0, s, "PragmaChange: readable release and 64-hex secret files are required");
  return HTTP_INTERNAL_SERVER_ERROR;
}
static int pc_post_config(apr_pool_t *p, apr_pool_t *log, apr_pool_t *temp, server_rec *s) {
  pc_server *c = ap_get_module_config(s->module_config, &pragmachange_module);
  pc_store *store = NULL;
  apr_status_t rv;
  pc_off shared;
  (void)p; (void)log; (void)temp;
  if (!c->runtime) return OK;
  /* process->pool outlives graceful configuration generations. */
  apr_pool_userdata_get((void **)&store, "pragmachange.history.v1", s->process->pool);
  if (store) {
    if (store->size != c->size) {
      ap_log_error(APLOG_MARK, APLOG_ERR, 0, s, "PragmaChange: changing state capacity requires a full restart");
      return HTTP_INTERNAL_SERVER_ERROR;
    }
  } else {
    apr_pool_t *lifetime = s->process->pool;
    store = apr_pcalloc(lifetime, sizeof(*store));
    store->size = c->size;
    store->lock_path = ap_runtime_dir_relative(lifetime, "pragmachange.lock");
    if (!store->lock_path) goto bad_config;
    rv = apr_global_mutex_create(&store->mutex, store->lock_path, APR_LOCK_DEFAULT, lifetime);
    if (rv != APR_SUCCESS) goto state_error;
#if AP_NEED_SET_MUTEX_PERMS
    rv = ap_unixd_set_global_mutex_perms(store->mutex);
    if (rv != APR_SUCCESS) goto state_error;
#endif
    rv = apr_shm_create(&store->shm, c->size, NULL, lifetime);
    if (rv != APR_SUCCESS) goto state_error;
    rv = apr_rmm_init(&store->rmm, NULL, apr_shm_baseaddr_get(store->shm), c->size, lifetime);
    if (rv != APR_SUCCESS) goto state_error;
    shared = apr_rmm_calloc(store->rmm, sizeof(pc_shared));
    if (!shared) { rv = APR_ENOMEM; goto state_error; }
    store->shared = apr_rmm_addr_get(store->rmm, shared);
    /* Copy the key: the DSO containing its string literal is unloaded
     * between startup passes and graceful generations. */
    apr_pool_userdata_set(store, "pragmachange.history.v1", NULL, lifetime);
  }
  c->store = store;
  /* One immutable runtime and history zone across all virtual hosts. */
  for (s = s->next; s; s = s->next) {
    pc_server *v = ap_get_module_config(s->module_config, &pragmachange_module);
    v->runtime = c->runtime; v->store = store;
    v->guard_index = c->guard_index; v->guard_max_body = c->guard_max_body;
  }
  return OK;
state_error:
  ap_log_error(APLOG_MARK, APLOG_ERR, rv, s, "PragmaChange: cannot initialize shared history");
  return HTTP_INTERNAL_SERVER_ERROR;
bad_config:
  ap_log_error(APLOG_MARK, APLOG_ERR, 0, s, "PragmaChange: readable release and 64-hex secret files are required");
  return HTTP_INTERNAL_SERVER_ERROR;
}
static void pc_child_init(apr_pool_t *p, server_rec *s) {
  pc_server *c = ap_get_module_config(s->module_config, &pragmachange_module);
  if (c->store && apr_global_mutex_child_init(&c->store->mutex, c->store->lock_path, p) != APR_SUCCESS) {
    ap_log_error(APLOG_MARK, APLOG_CRIT, 0, s, "PragmaChange: child cannot attach shared mutex");
    exit(APEXIT_CHILDFATAL);
  }
}
/* A bounded prefix is replayed above Apache's body decoder. The unread suffix
 * remains in the original filter chain, including after an over-limit check. */
static apr_status_t pc_replay(ap_filter_t *f, apr_bucket_brigade *bb,
                             ap_input_mode_t mode, apr_read_type_e block, apr_off_t bytes) {
  pc_request *ctx = f->ctx;
  size_t n = ctx->length - ctx->position;
  if (mode == AP_MODE_INIT) return APR_SUCCESS;
  if (n) {
    if ((mode == AP_MODE_READBYTES || mode == AP_MODE_SPECULATIVE) && bytes > 0 && (apr_uint64_t)bytes < n) n = (size_t)bytes;
    if (mode == AP_MODE_GETLINE) {
      unsigned char *nl = memchr(ctx->body + ctx->position, '\n', n);
      if (nl) n = (size_t)(nl - ctx->body - ctx->position) + 1;
    }
    APR_BRIGADE_INSERT_TAIL(bb, apr_bucket_pool_create((char *)ctx->body + ctx->position, n, f->r->pool, f->c->bucket_alloc));
    if (mode != AP_MODE_SPECULATIVE) ctx->position += n;
    return APR_SUCCESS;
  }
  if (ctx->complete) {
    APR_BRIGADE_INSERT_TAIL(bb, apr_bucket_eos_create(f->c->bucket_alloc));
    return APR_SUCCESS;
  }
  return ap_get_brigade(f->next, bb, mode, block, bytes);
}
static int pc_read_body(request_rec *r, pc_request *ctx, size_t maximum) {
  apr_bucket_brigade *bb = apr_brigade_create(r->pool, r->connection->bucket_alloc);
  apr_status_t rv;
  size_t capacity = maximum + 1;
  ctx->body = apr_palloc(r->pool, capacity);
  while (ctx->length < capacity && !ctx->complete) {
    apr_bucket *b;
    size_t want = capacity - ctx->length;
    if (want > 8192) want = 8192;
    rv = ap_get_brigade(r->input_filters, bb, AP_MODE_READBYTES, APR_BLOCK_READ, (apr_off_t)want);
    if (rv != APR_SUCCESS) return ap_map_http_request_error(rv, HTTP_BAD_REQUEST);
    if (APR_BRIGADE_EMPTY(bb)) return HTTP_BAD_REQUEST;
    for (b = APR_BRIGADE_FIRST(bb); b != APR_BRIGADE_SENTINEL(bb); b = APR_BUCKET_NEXT(b)) {
      const char *data;
      apr_size_t n;
      if (APR_BUCKET_IS_EOS(b)) { ctx->complete = 1; continue; }
      if (APR_BUCKET_IS_METADATA(b)) continue;
      rv = apr_bucket_read(b, &data, &n, APR_BLOCK_READ);
      if (rv != APR_SUCCESS) return ap_map_http_request_error(rv, HTTP_BAD_REQUEST);
      /* A third-party input filter that ignores the read bound cannot be
       * replayed safely within our memory contract. Do not forward partial data. */
      if (n > capacity - ctx->length) return HTTP_INTERNAL_SERVER_ERROR;
      memcpy(ctx->body + ctx->length, data, n);
      ctx->length += n;
    }
    apr_brigade_cleanup(bb);
  }
  ap_add_input_filter("PRAGMACHANGE_REPLAY", ctx, r, r->connection);
  return OK;
}
static void pc_strip(request_rec *r) {
  const apr_array_header_t *array = apr_table_elts(r->headers_in);
  const apr_table_entry_t *entries = (const apr_table_entry_t *)array->elts;
  int i;
  /* Unset mutates the array; restart to also remove case variants. */
  for (i = 0; i < array->nelts; ++i)
    if (entries[i].key && !strncasecmp(entries[i].key, "X-PragmaChange-", sizeof("X-PragmaChange-") - 1)) {
      const char *key = apr_pstrdup(r->pool, entries[i].key);
      apr_table_unset(r->headers_in, key);
      array = apr_table_elts(r->headers_in);
      entries = (const apr_table_entry_t *)array->elts;
      i = -1;
    }
}
static int pc_block(request_rec *r, const char *decision, const char *reason) {
  apr_table_setn(r->err_headers_out, "X-PragmaChange-Decision", decision);
  apr_table_set(r->err_headers_out, "X-PragmaChange-Reason", reason);
  return HTTP_FORBIDDEN;
}
static int pc_inspect(request_rec *r) {
  pc_location *location = ap_get_module_config(r->per_dir_config, &pragmachange_module);
  pc_server *c = ap_get_module_config(r->server->module_config, &pragmachange_module);
  pc_request *ctx;
  PcRequest request = {0};
  PcDecision decision = {0};
  PcGuardDecision guard = {0};
  PcPrepared *prepared;
  const apr_array_header_t *array;
  const apr_table_entry_t *entries;
  PcHeader headers[256];
  size_t count = 0, maximum = 0;
  uint32_t needs_body = 0;
  int32_t index;
  int i, result, full_body;
  struct timespec now;
  apr_status_t rv;
  if (r->main || !location || location->enabled != 1) return DECLINED;
  ctx = ap_get_module_config(r->request_config, &pragmachange_module);
  if (ctx && ctx->inspected) return DECLINED;
  pc_strip(r);
  if (!c->runtime || !c->store) return HTTP_SERVICE_UNAVAILABLE;
  /* The same request inspection runs before a local handler such as mod_php
   * or before a reverse-proxy handler. Both consume the replayed body. */
  if (r->proxyreq != PROXYREQ_NONE && r->proxyreq != PROXYREQ_REVERSE) return DECLINED;
  index = pc_route(c->runtime, pc_string(r->method), pc_string(r->uri), pc_string(r->hostname), &needs_body, &maximum);
  if (index == -1 && c->guard_index < 0) return DECLINED;
  if (index < -1 || (index >= 0 && maximum > PC_MAX_FILE)) return HTTP_INTERNAL_SERVER_ERROR;
  ctx = apr_pcalloc(r->pool, sizeof(*ctx));
  ap_set_module_config(r->request_config, &pragmachange_module, ctx);
  ctx->inspected = 1;
  request.method = pc_string(r->method); request.path = pc_string(r->uri);
  request.host = pc_string(r->hostname); request.query = pc_string(r->args);
  request.client_ip = pc_string(r->useragent_ip);
  full_body = index >= 0 ? pc_route_full_body(c->runtime, index) : 0;
  if (full_body < 0) return HTTP_INTERNAL_SERVER_ERROR;
  if (c->guard_index >= 0 || needs_body) {
    size_t limit = c->guard_index >= 0 ? c->guard_max_body : maximum;
    result = pc_read_body(r, ctx, limit);
    if (result != OK) return result;
    request.body_size = ctx->length;
    if (c->guard_index >= 0) {
      const char *content_type = apr_table_get(r->headers_in, "Content-Type");
      if (ctx->length > c->guard_max_body || !ctx->complete)
        return pc_block(r, "guard_block", "body_limit");
      if (pc_guard_decide(c->runtime, c->guard_index, pc_string(content_type ? content_type : ""),
                          (PcSlice){ctx->body, ctx->length}, &guard) != 0)
        return HTTP_INTERNAL_SERVER_ERROR;
      if (guard.action == 2)
        return pc_block(r, "guard_block", (const char *)guard.reason);
      request.has_multipart_metrics = guard.has_multipart_metrics;
      request.multipart_parts = guard.multipart_parts;
      request.multipart_file_parts = guard.multipart_file_parts;
      request.multipart_max_part_headers = guard.multipart_max_part_headers;
      request.multipart_max_field_nesting = guard.multipart_max_field_nesting;
      request.multipart_total_header_bytes = guard.multipart_total_header_bytes;
      request.multipart_empty_file_parts = guard.multipart_empty_file_parts;
    } else if (ctx->length > maximum || !ctx->complete) request.input_error = 1;
    if (full_body) {
      if (ctx->length > maximum) request.input_error = 1;
      else request.body = (PcSlice){ctx->body, ctx->length};
    }
  }
  /* Body decoders may merge trailers into headers_in. Strip spoofed metadata
   * before returning even when no model route matched. */
  pc_strip(r);
  if (c->guard_index >= 0)
    apr_table_setn(r->headers_in, "X-PragmaChange-Guard", "allow");
  if (index == -1) return DECLINED;
  /* Apache merges repeated header lines. Treat ambiguous values as duplicates;
   * the Rust adapter rejects them only when that header is selected. Cookie's
   * HTTP/2 merge separator is ';'. Unselected headers remain ignored. */
  array = apr_table_elts(r->headers_in);
  entries = (const apr_table_entry_t *)array->elts;
  for (i = 0; i < array->nelts; ++i) {
    int copies, j;
    if (!entries[i].key) continue;
    copies = strchr(entries[i].val, ',') || (!strcasecmp(entries[i].key, "Cookie") && strchr(entries[i].val, ';')) ? 2 : 1;
    for (j = 0; j < copies; ++j) {
      if (count == 256) { request.input_error = 1; break; }
      headers[count++] = (PcHeader){pc_string(entries[i].key), pc_string(entries[i].val)};
    }
  }
  request.headers = headers; request.header_count = count;
  /* No state update twice across an internal redirect. A changed final route
   * receives an explicit unscored check rather than another history event. */
  if (r->prev) {
    request_rec *previous;
    for (previous = r->prev; previous; previous = previous->prev) {
      pc_request *old = ap_get_module_config(previous->request_config, &pragmachange_module);
      if (old && old->inspected) request.input_error = 1;
    }
  }
  rv = apr_global_mutex_lock(c->store->mutex);
  if (rv != APR_SUCCESS) return HTTP_SERVICE_UNAVAILABLE;
  if (clock_gettime(CLOCK_MONOTONIC, &now)) {
    apr_global_mutex_unlock(c->store->mutex); return HTTP_INTERNAL_SERVER_ERROR;
  }
  request.timestamp_ms = (uint64_t)now.tv_sec * 1000 + (uint64_t)now.tv_nsec / 1000000;
  prepared = pc_features(c->runtime, index, &request, pc_get, pc_put, c->store);
  rv = apr_global_mutex_unlock(c->store->mutex);
  if (!prepared || rv != APR_SUCCESS) { if (prepared) pc_prepared_free(prepared); return HTTP_INTERNAL_SERVER_ERROR; }
  result = pc_decide(c->runtime, prepared, &decision);
  pc_prepared_free(prepared);
  if (result != 0 || decision.action > 2) return HTTP_INTERNAL_SERVER_ERROR;
  if (decision.action == 2) {
    if (decision.has_score)
      apr_table_setn(r->err_headers_out, "X-PragmaChange-Score", apr_psprintf(r->pool, "%.17g", decision.score));
    apr_table_set(r->err_headers_out, "X-PragmaChange-Model", (const char *)decision.model_version_id);
    apr_table_set(r->err_headers_out, "X-PragmaChange-Pipeline", (const char *)decision.pipeline_version_id);
    return pc_block(r, "model_block", (const char *)decision.reason);
  }
  apr_table_setn(r->headers_in, "X-PragmaChange-Action", decision.action ? "check" : "allow");
  apr_table_setn(r->headers_in, "X-PragmaChange-Score", decision.has_score ? apr_psprintf(r->pool, "%.17g", decision.score) : "");
  apr_table_set(r->headers_in, "X-PragmaChange-Reason", (const char *)decision.reason);
  apr_table_set(r->headers_in, "X-PragmaChange-Pipeline", (const char *)decision.pipeline_version_id);
  apr_table_set(r->headers_in, "X-PragmaChange-Model", (const char *)decision.model_version_id);
  return DECLINED;
}
static int pc_loopback(apr_sockaddr_t *addr) {
  char *ip = NULL;
  if (!addr || apr_sockaddr_ip_get(&ip, addr) != APR_SUCCESS) return 0;
  return !strcmp(ip, "::1") || !strncmp(ip, "127.", 4);
}
static int pc_status(request_rec *r) {
  pc_server *c;
  uint8_t id[37];
  if (!r->handler || strcmp(r->handler, "pragmachange-status")) return DECLINED;
  /* Check actual socket addresses, never mod_remoteip's rewritten user IP. */
  if (!pc_loopback(r->connection->local_addr) || !pc_loopback(r->connection->client_addr)) return HTTP_FORBIDDEN;
  if (r->method_number != M_GET) return HTTP_METHOD_NOT_ALLOWED;
  c = ap_get_module_config(r->server->module_config, &pragmachange_module);
  if (!c->runtime || pc_release_id(c->runtime, id) != 0) return HTTP_SERVICE_UNAVAILABLE;
  ap_set_content_type(r, "application/json");
  apr_table_setn(r->headers_out, "Cache-Control", "no-store");
  ap_rprintf(r, "{\"release_id\":\"%s\",\"module_abi\":1,\"module_version\":\"%s\",\"pid\":%ld}\n", id, PC_MODULE_VERSION, (long)getpid());
  return OK;
}
static void pc_hooks(apr_pool_t *p) {
  (void)p;
  ap_hook_pre_config(pc_pre_config, NULL, NULL, APR_HOOK_MIDDLE);
  ap_hook_check_config(pc_check_config, NULL, NULL, APR_HOOK_MIDDLE);
  ap_hook_post_config(pc_post_config, NULL, NULL, APR_HOOK_MIDDLE);
  ap_hook_child_init(pc_child_init, NULL, NULL, APR_HOOK_MIDDLE);
  ap_hook_fixups(pc_inspect, NULL, NULL, APR_HOOK_LAST);
  ap_hook_handler(pc_status, NULL, NULL, APR_HOOK_FIRST);
  ap_register_input_filter("PRAGMACHANGE_REPLAY", pc_replay, NULL, AP_FTYPE_RESOURCE);
}
module AP_MODULE_DECLARE_DATA pragmachange_module = {
  STANDARD20_MODULE_STUFF, pc_create_location, pc_merge_location,
  pc_create_server, NULL, pc_commands, pc_hooks, AP_MODULE_FLAG_NONE
};
