#include <ngx_config.h>
#include <ngx_core.h>
#include <ngx_http.h>
#include "pragmachange.h"

/* Shared state contains only slab pointers (same mapping in workers), never
 * Rust objects. */
typedef struct {
  ngx_rbtree_node_t node;
  ngx_queue_t lru;
  u_char key[101];
  size_t len;
  u_char *data;
} pc_entry;
typedef struct {
  ngx_rbtree_t tree;
  ngx_rbtree_node_t sentinel;
  ngx_queue_t lru;
  ngx_uint_t count;
} pc_shared;
typedef struct {
  ngx_str_t release_path, secret_path;
  ngx_shm_zone_t *zone;
  ngx_slab_pool_t *pool;
  pc_shared *shared;
  PcRuntime *runtime;
} pc_main;
typedef struct {
  ngx_flag_t enabled;
} pc_loc;
typedef struct {
  ngx_int_t status;
  int32_t index;
  size_t max_body;
} pc_ctx;
static ngx_int_t pc_init(ngx_conf_t *cf);
static char *pc_status_directive(ngx_conf_t *cf, ngx_command_t *cmd, void *conf);
static void *pc_create_main(ngx_conf_t *cf);
static void *pc_create_loc(ngx_conf_t *cf);
static char *pc_merge_loc(ngx_conf_t *cf, void *parent, void *child);
static char *pc_zone(ngx_conf_t *cf, ngx_command_t *cmd, void *conf);
static ngx_int_t pc_handler(ngx_http_request_t *r);
static ngx_int_t pc_inspect(ngx_http_request_t *r, pc_ctx *ctx);
static void pc_body_ready(ngx_http_request_t *r);
extern ngx_module_t ngx_http_pragmachange_module;
static ngx_command_t pc_commands[] = {
    {ngx_string("pragmachange_zone"), NGX_HTTP_MAIN_CONF | NGX_CONF_TAKE1,
     pc_zone, NGX_HTTP_MAIN_CONF_OFFSET, 0, NULL},
    {ngx_string("pragmachange_release"), NGX_HTTP_MAIN_CONF | NGX_CONF_TAKE1,
     ngx_conf_set_str_slot, NGX_HTTP_MAIN_CONF_OFFSET,
     offsetof(pc_main, release_path), NULL},
    {ngx_string("pragmachange_secret_file"),
     NGX_HTTP_MAIN_CONF | NGX_CONF_TAKE1, ngx_conf_set_str_slot,
     NGX_HTTP_MAIN_CONF_OFFSET, offsetof(pc_main, secret_path), NULL},
    {ngx_string("pragmachange"),
     NGX_HTTP_LOC_CONF | NGX_HTTP_SRV_CONF | NGX_CONF_FLAG,
     ngx_conf_set_flag_slot, NGX_HTTP_LOC_CONF_OFFSET,
     offsetof(pc_loc, enabled), NULL},
    {ngx_string("pragmachange_status"), NGX_HTTP_LOC_CONF | NGX_CONF_NOARGS,
     pc_status_directive, NGX_HTTP_LOC_CONF_OFFSET, 0, NULL},
    ngx_null_command};
static ngx_http_module_t pc_module_ctx = {
    NULL, pc_init, pc_create_main, NULL,
    NULL, NULL,    pc_create_loc,  pc_merge_loc};
ngx_module_t ngx_http_pragmachange_module = {
    NGX_MODULE_V1, &pc_module_ctx, pc_commands, NGX_HTTP_MODULE,
    NULL,          NULL,           NULL,        NULL,
    NULL,          NULL,           NULL,        NGX_MODULE_V1_PADDING};
static PcSlice pc_slice(ngx_str_t value) {
  PcSlice s = {value.data, value.len};
  return s;
}
static void *pc_create_main(ngx_conf_t *cf) {
  return ngx_pcalloc(cf->pool, sizeof(pc_main));
}
static void *pc_create_loc(ngx_conf_t *cf) {
  pc_loc *c = ngx_pcalloc(cf->pool, sizeof(pc_loc));
  if (c)
    c->enabled = NGX_CONF_UNSET;
  return c;
}
static char *pc_merge_loc(ngx_conf_t *cf, void *parent, void *child) {
  pc_loc *p = parent, *c = child;
  pc_main *m =
      ngx_http_conf_get_module_main_conf(cf, ngx_http_pragmachange_module);
  ngx_conf_merge_value(c->enabled, p->enabled, 0);
  if (c->enabled && !m->release_path.len)
    return "pragmachange on requires pragmachange_release";
  return NGX_CONF_OK;
}
static void pc_tree_insert(ngx_rbtree_node_t *root, ngx_rbtree_node_t *node,
                           ngx_rbtree_node_t *sentinel) {
  ngx_rbtree_node_t **p;
  for (;;) {
    if (node->key < root->key)
      p = &root->left;
    else if (node->key > root->key)
      p = &root->right;
    else
      p = ngx_memcmp(((pc_entry *)node)->key, ((pc_entry *)root)->key, 101) < 0
              ? &root->left
              : &root->right;
    if (*p == sentinel) {
      break;
    }
    root = *p;
  }
  *p = node;
  node->parent = root;
  node->left = sentinel;
  node->right = sentinel;
  ngx_rbt_red(node);
}
static ngx_int_t pc_zone_init(ngx_shm_zone_t *zone, void *old) {
  pc_main *m = zone->data, *previous = old;
  m->pool = (ngx_slab_pool_t *)zone->shm.addr;
  if (previous) {
    m->shared = previous->shared;
    return NGX_OK;
  }
  if (zone->shm.exists) {
    m->shared = m->pool->data;
    return NGX_OK;
  }
  m->shared = ngx_slab_alloc(m->pool, sizeof(pc_shared));
  if (!m->shared)
    return NGX_ERROR;
  ngx_memzero(m->shared, sizeof(pc_shared));
  m->pool->data = m->shared;
  ngx_rbtree_init(&m->shared->tree, &m->shared->sentinel, pc_tree_insert);
  ngx_queue_init(&m->shared->lru);
  return NGX_OK;
}
static char *pc_zone(ngx_conf_t *cf, ngx_command_t *cmd, void *conf) {
  pc_main *m = conf;
  ngx_str_t *args = cf->args->elts;
  ngx_str_t name = ngx_string("pragmachange_state_v1");
  ssize_t size = ngx_parse_size(&args[1]);
  (void)cmd;
  if (m->zone) {
    return "is duplicate";
  }
  if (size < 1024 * 1024) {
    return "must be at least 1m";
  }
  m->zone = ngx_shared_memory_add(cf, &name, (size_t)size,
                                  &ngx_http_pragmachange_module);
  if (!m->zone)
    return NGX_CONF_ERROR;
  m->zone->init = pc_zone_init;
  m->zone->data = m;
  return NGX_CONF_OK;
}
static pc_entry *pc_find(pc_main *m, PcSlice key) {
  ngx_rbtree_node_t *n = m->shared->tree.root,
                    *sentinel = m->shared->tree.sentinel;
  ngx_rbtree_key_t hash = ngx_crc32_short((u_char *)key.data, key.len);
  int cmp;
  while (n != sentinel) {
    if (hash < n->key)
      n = n->left;
    else if (hash > n->key)
      n = n->right;
    else {
      cmp = ngx_memcmp(key.data, ((pc_entry *)n)->key, 101);
      if (cmp == 0)
        return (pc_entry *)n;
      n = cmp < 0 ? n->left : n->right;
    }
  }
  return NULL;
}
static void pc_remove(pc_main *m, pc_entry *entry) {
  ngx_rbtree_delete(&m->shared->tree, &entry->node);
  ngx_queue_remove(&entry->lru);
  ngx_slab_free_locked(m->pool, entry->data);
  ngx_slab_free_locked(m->pool, entry);
  m->shared->count--;
}
static ngx_int_t pc_evict(pc_main *m, pc_entry *keep) {
  ngx_queue_t *q;
  pc_entry *entry;
  for (q = ngx_queue_last(&m->shared->lru);
       q != ngx_queue_sentinel(&m->shared->lru); q = ngx_queue_prev(q)) {
    entry = ngx_queue_data(q, pc_entry, lru);
    if (entry != keep) {
      pc_remove(m, entry);
      return NGX_OK;
    }
  }
  return NGX_ERROR;
}
static int32_t pc_get(void *context, PcSlice key, PcSlice *out) {
  pc_main *m = context;
  pc_entry *entry;
  if (key.len != 101) {
    return -1;
  }
  entry = pc_find(m, key);
  if (!entry)
    return 0;
  out->data = entry->data;
  out->len = entry->len;
  return 1;
}
static ngx_uint_t pc_namespace_count(pc_main *m, PcSlice key) {
  ngx_queue_t *q;
  pc_entry *entry;
  ngx_uint_t count = 0;
  for (q = ngx_queue_head(&m->shared->lru);
       q != ngx_queue_sentinel(&m->shared->lru); q = ngx_queue_next(q)) {
    entry = ngx_queue_data(q, pc_entry, lru);
    if (ngx_memcmp(entry->key, key.data, 36) == 0)
      count++;
  }
  return count;
}
static ngx_int_t pc_evict_namespace(pc_main *m, PcSlice key) {
  ngx_queue_t *q;
  pc_entry *entry;
  for (q = ngx_queue_last(&m->shared->lru);
       q != ngx_queue_sentinel(&m->shared->lru); q = ngx_queue_prev(q)) {
    entry = ngx_queue_data(q, pc_entry, lru);
    if (ngx_memcmp(entry->key, key.data, 36) == 0) {
      pc_remove(m, entry);
      return NGX_OK;
    }
  }
  return NGX_ERROR;
}
static int32_t pc_put(void *context, PcSlice key, PcSlice value,
                      size_t max_partitions) {
  pc_main *m = context;
  pc_entry *entry;
  u_char *data;
  if (key.len != 101) {
    return -1;
  }
  entry = pc_find(m, key);
  if (!entry && pc_namespace_count(m, key) >= max_partitions) {
    if (pc_evict_namespace(m, key) != NGX_OK)
      return -1;
  }
  data = ngx_slab_alloc_locked(m->pool, value.len);
  while (!data && pc_evict(m, entry) == NGX_OK)
    data = ngx_slab_alloc_locked(m->pool, value.len);
  if (!data) {
    return -1;
  }
  ngx_memcpy(data, value.data, value.len);
  if (!entry) {
    entry = ngx_slab_alloc_locked(m->pool, sizeof(pc_entry));
    while (!entry && pc_evict(m, NULL) == NGX_OK)
      entry = ngx_slab_alloc_locked(m->pool, sizeof(pc_entry));
    if (!entry) {
      ngx_slab_free_locked(m->pool, data);
      return -1;
    }
    ngx_memzero(entry, sizeof(pc_entry));
    ngx_memcpy(entry->key, key.data, 101);
    entry->node.key = ngx_crc32_short((u_char *)key.data, key.len);
    ngx_rbtree_insert(&m->shared->tree, &entry->node);
    m->shared->count++;
  } else {
    ngx_queue_remove(&entry->lru);
    ngx_slab_free_locked(m->pool, entry->data);
  }
  entry->data = data;
  entry->len = value.len;
  ngx_queue_insert_head(&m->shared->lru, &entry->lru);
  return 0;
}
static ngx_int_t pc_read_file(ngx_conf_t *cf, ngx_str_t path,
                              ngx_str_t *content, size_t maximum) {
  ngx_file_t file;
  ngx_file_info_t info;
  ssize_t size;
  ngx_memzero(&file, sizeof(file));
  file.fd = ngx_open_file(path.data, NGX_FILE_RDONLY, NGX_FILE_OPEN, 0);
  file.log = cf->log;
  if (file.fd == NGX_INVALID_FILE)
    return NGX_ERROR;
  if (ngx_fd_info(file.fd, &info) == NGX_FILE_ERROR) {
    ngx_close_file(file.fd);
    return NGX_ERROR;
  }
  size = ngx_file_size(&info);
  if (size <= 0 || (size_t)size > maximum) {
    ngx_close_file(file.fd);
    return NGX_ERROR;
  }
  content->data = ngx_pnalloc(cf->pool, (size_t)size);
  content->len = (size_t)size;
  if (!content->data ||
      ngx_read_file(&file, content->data, content->len, 0) != size) {
    ngx_close_file(file.fd);
    return NGX_ERROR;
  }
  ngx_close_file(file.fd);
  return NGX_OK;
}
static void pc_runtime_cleanup(void *data) { pc_runtime_free(data); }
static ngx_int_t pc_init(ngx_conf_t *cf) {
  pc_main *m =
      ngx_http_conf_get_module_main_conf(cf, ngx_http_pragmachange_module);
  ngx_http_core_main_conf_t *core;
  ngx_http_handler_pt *handler;
  ngx_str_t release, secret;
  u_char raw[32], error[256];
  ngx_int_t hi, lo;
  ngx_uint_t i;
  ngx_pool_cleanup_t *cleanup;
  if (!m->release_path.len) {
    return NGX_OK;
  }
  if (!m->zone || !m->secret_path.len) {
    ngx_conf_log_error(NGX_LOG_EMERG, cf, 0,
                       "pragmachange requires zone and secret_file");
    return NGX_ERROR;
  }
  if (pc_read_file(cf, m->release_path, &release, 64 * 1024 * 1024) != NGX_OK ||
      pc_read_file(cf, m->secret_path, &secret, 128) != NGX_OK) {
    ngx_conf_log_error(NGX_LOG_EMERG, cf, 0,
                       "cannot read pragmachange release/secret");
    return NGX_ERROR;
  }
  while (secret.len && (secret.data[secret.len - 1] == '\n' ||
                        secret.data[secret.len - 1] == '\r'))
    secret.len--;
  if (secret.len != 64) {
    ngx_conf_log_error(NGX_LOG_EMERG, cf, 0,
                       "partition secret must contain 64 hex characters");
    return NGX_ERROR;
  }
  for (i = 0; i < 32; i++) {
    hi = ngx_hextoi(secret.data + 2 * i, 1);
    lo = ngx_hextoi(secret.data + 2 * i + 1, 1);
    if (hi == NGX_ERROR || lo == NGX_ERROR)
      return NGX_ERROR;
    raw[i] = (u_char)(hi * 16 + lo);
  }
  m->runtime = pc_runtime_new(pc_slice(release), (PcSlice){raw, 32}, error);
  ngx_explicit_memzero(raw, 32);
  if (!m->runtime) {
    ngx_conf_log_error(NGX_LOG_EMERG, cf, 0, "invalid PragmaChange release: %s",
                       error);
    return NGX_ERROR;
  }
  cleanup = ngx_pool_cleanup_add(cf->pool, 0);
  if (!cleanup) {
    pc_runtime_free(m->runtime);
    return NGX_ERROR;
  }
  cleanup->handler = pc_runtime_cleanup;
  cleanup->data = m->runtime;
  core = ngx_http_conf_get_module_main_conf(cf, ngx_http_core_module);
  handler = ngx_array_push(&core->phases[NGX_HTTP_PREACCESS_PHASE].handlers);
  if (!handler)
    return NGX_ERROR;
  *handler = pc_handler;
  return NGX_OK;
}
static ngx_int_t pc_strip_headers(ngx_http_request_t *r) {
  ngx_list_t filtered;
  ngx_list_part_t *part = &r->headers_in.headers.part;
  ngx_table_elt_t *h = part->elts, *copy;
  ngx_uint_t i;
  /* proxy_pass does not skip input headers with hash=0. Build a filtered list;
     preserve the original structs so Nginx's dedicated header pointers remain
     valid. */
  if (ngx_list_init(&filtered, r->pool, 20, sizeof(ngx_table_elt_t)) != NGX_OK)
    return NGX_ERROR;
  for (i = 0;; i++) {
    if (i >= part->nelts) {
      if (!part->next)
        break;
      part = part->next;
      h = part->elts;
      i = 0;
    }
    if (h[i].key.len >= 15 &&
        ngx_strncasecmp(h[i].key.data, (u_char *)"X-PragmaChange-", 15) == 0)
      continue;
    copy = ngx_list_push(&filtered);
    if (!copy)
      return NGX_ERROR;
    *copy = h[i];
  }
  r->headers_in.headers = filtered;
  if (filtered.last == &filtered.part)
    r->headers_in.headers.last = &r->headers_in.headers.part;
  return NGX_OK;
}
static ngx_int_t pc_add_header(ngx_http_request_t *r, const char *name,
                               const char *value) {
  ngx_table_elt_t *h = ngx_list_push(&r->headers_in.headers);
  size_t n = ngx_strlen(name), v = ngx_strlen(value);
  if (!h)
    return NGX_ERROR;
  ngx_memzero(h, sizeof(*h));
  h->key.len = n;
  h->key.data = ngx_pnalloc(r->pool, n);
  h->value.len = v;
  h->value.data = ngx_pnalloc(r->pool, v ? v : 1);
  h->lowcase_key = ngx_pnalloc(r->pool, n);
  if (!h->key.data || !h->value.data || !h->lowcase_key)
    return NGX_ERROR;
  ngx_memcpy(h->key.data, name, n);
  ngx_memcpy(h->value.data, value, v);
  h->hash = ngx_hash_strlow(h->lowcase_key, h->key.data, n);
  return NGX_OK;
}
static ngx_int_t pc_inspect(ngx_http_request_t *r, pc_ctx *ctx) {
  pc_main *m = ngx_http_get_module_main_conf(r, ngx_http_pragmachange_module);
  PcRequest request;
  PcHeader *headers;
  PcPrepared *prepared;
  PcDecision decision;
  ngx_list_part_t *part;
  ngx_table_elt_t *h;
  ngx_uint_t i, count = 0;
  ngx_chain_t *chain;
  size_t total = 0;
  u_char *body = NULL, *cursor;
  char score[48];
  ngx_memzero(&request, sizeof(request));
  ngx_memzero(&decision, sizeof(decision));
  request.timestamp_ms = (uint64_t)ngx_current_msec;
  request.method = pc_slice(r->method_name);
  request.path = pc_slice(r->uri);
  request.host = pc_slice(r->headers_in.server);
  request.query = pc_slice(r->args);
  request.client_ip = pc_slice(r->connection->addr_text);
  headers = ngx_pcalloc(r->pool, 256 * sizeof(PcHeader));
  if (!headers)
    return NGX_HTTP_INTERNAL_SERVER_ERROR;
  part = &r->headers_in.headers.part;
  h = part->elts;
  for (i = 0;; i++) {
    if (i >= part->nelts) {
      if (!part->next)
        break;
      part = part->next;
      h = part->elts;
      i = 0;
    }
    if (!h[i].hash)
      continue;
    if (count >= 256) {
      request.input_error = 1;
      break;
    }
    headers[count].name = pc_slice(h[i].key);
    headers[count].value = pc_slice(h[i].value);
    count++;
  }
  request.headers = headers;
  request.header_count = count;
  if (r->request_body) {
    for (chain = r->request_body->bufs; chain; chain = chain->next) {
      if (chain->buf->in_file)
        request.input_error = 1;
      total += (size_t)ngx_buf_size(chain->buf);
    }
    if (r->request_body->temp_file)
      request.input_error = 1;
    if (total > ctx->max_body)
      request.input_error = 1;
    if (!request.input_error && total) {
      body = ngx_pnalloc(r->pool, total);
      if (!body)
        return NGX_HTTP_INTERNAL_SERVER_ERROR;
      cursor = body;
      for (chain = r->request_body->bufs; chain; chain = chain->next)
        cursor = ngx_cpymem(cursor, chain->buf->pos,
                            (size_t)ngx_buf_size(chain->buf));
    }
  }
  request.body_size = r->headers_in.content_length_n >= 0
                          ? (uint64_t)r->headers_in.content_length_n
                          : total;
  request.body = (PcSlice){body, body ? total : 0};
  ngx_shmtx_lock(&m->pool->mutex);
  prepared = pc_features(m->runtime, ctx->index, &request, pc_get, pc_put, m);
  ngx_shmtx_unlock(&m->pool->mutex);
  if (!prepared || pc_decide(m->runtime, prepared, &decision) != 0) {
    decision.action = 1;
    ngx_cpystrn(decision.reason, (u_char *)"runtime_error",
                sizeof(decision.reason));
  }
  pc_prepared_free(prepared);
  if (decision.action == 2)
    return NGX_HTTP_FORBIDDEN;
  score[0] = '\0';
  if (decision.has_score)
    snprintf(score, sizeof(score), "%.17g", decision.score);
  if (pc_add_header(r, "X-PragmaChange-Action",
                    decision.action == 0 ? "allow" : "check") != NGX_OK ||
      pc_add_header(r, "X-PragmaChange-Score", score) != NGX_OK ||
      pc_add_header(r, "X-PragmaChange-Reason", (char *)decision.reason) !=
          NGX_OK ||
      pc_add_header(r, "X-PragmaChange-Pipeline",
                    (char *)decision.pipeline_version_id) != NGX_OK ||
      pc_add_header(r, "X-PragmaChange-Model",
                    (char *)decision.model_version_id) != NGX_OK)
    return NGX_HTTP_INTERNAL_SERVER_ERROR;
  return NGX_DECLINED;
}
static ngx_int_t pc_handler(ngx_http_request_t *r) {
  pc_loc *loc = ngx_http_get_module_loc_conf(r, ngx_http_pragmachange_module);
  pc_main *m;
  pc_ctx *ctx;
  uint32_t body;
  size_t max_body;
  int32_t index;
  ngx_int_t rc;
  if (!loc->enabled || r != r->main)
    return NGX_DECLINED;
  ctx = ngx_http_get_module_ctx(r, ngx_http_pragmachange_module);
  if (ctx)
    return ctx->status;
  if (pc_strip_headers(r) != NGX_OK)
    return NGX_HTTP_INTERNAL_SERVER_ERROR;
  m = ngx_http_get_module_main_conf(r, ngx_http_pragmachange_module);
  if (!m->runtime)
    return NGX_HTTP_SERVICE_UNAVAILABLE;
  index = pc_route(m->runtime, pc_slice(r->method_name), pc_slice(r->uri),
                   pc_slice(r->headers_in.server), &body, &max_body);
  if (index == -1)
    return NGX_DECLINED;
  if (index < 0)
    return NGX_HTTP_BAD_REQUEST;
  ctx = ngx_pcalloc(r->pool, sizeof(pc_ctx));
  if (!ctx)
    return NGX_HTTP_INTERNAL_SERVER_ERROR;
  ctx->index = index;
  ctx->max_body = max_body;
  ctx->status = NGX_DONE;
  ngx_http_set_ctx(r, ctx, ngx_http_pragmachange_module);
  if (body) {
    rc = ngx_http_read_client_request_body(r, pc_body_ready);
    if (rc >= NGX_HTTP_SPECIAL_RESPONSE)
      return rc;
    ngx_http_finalize_request(r, NGX_DONE);
    return NGX_DONE;
  }
  ctx->status = pc_inspect(r, ctx);
  return ctx->status;
}
static void pc_body_ready(ngx_http_request_t *r) {
  pc_ctx *ctx = ngx_http_get_module_ctx(r, ngx_http_pragmachange_module);
  ctx->status = pc_inspect(r, ctx);
  r->preserve_body = 1;
  r->write_event_handler = ngx_http_core_run_phases;
  ngx_http_core_run_phases(r);
}

/* Identity is available only on a Unix listener. It is never a public header. */
static ngx_int_t pc_status_handler(ngx_http_request_t *r) {
  pc_main *m;
  u_char id[37], *end;
  ngx_buf_t *b;
  ngx_chain_t out;
  ngx_int_t rc;
  if (ngx_connection_local_sockaddr(r->connection, NULL, 0) != NGX_OK ||
      r->connection->local_sockaddr->sa_family != AF_UNIX)
    return NGX_HTTP_NOT_FOUND;
  if (!(r->method & (NGX_HTTP_GET | NGX_HTTP_HEAD)))
    return NGX_HTTP_NOT_ALLOWED;
  m = ngx_http_get_module_main_conf(r, ngx_http_pragmachange_module);
  if (!m || !m->runtime || pc_release_id(m->runtime, id) != 0)
    return NGX_HTTP_SERVICE_UNAVAILABLE;
  b = ngx_create_temp_buf(r->pool, 192);
  if (!b) return NGX_HTTP_INTERNAL_SERVER_ERROR;
  end = ngx_snprintf(b->pos, 192,
      "{\"release_id\":\"%s\",\"module_abi\":1,\"worker_pid\":%P}", id, ngx_pid);
  b->last = end;
  b->last_buf = (r == r->main);
  r->headers_out.status = NGX_HTTP_OK;
  ngx_str_set(&r->headers_out.content_type, "application/json");
  r->headers_out.content_length_n = end - b->pos;
  rc = ngx_http_send_header(r);
  if (rc == NGX_ERROR || rc > NGX_OK || r->header_only) return rc;
  out.buf = b; out.next = NULL;
  return ngx_http_output_filter(r, &out);
}
static char *pc_status_directive(ngx_conf_t *cf, ngx_command_t *cmd, void *conf) {
  ngx_http_core_loc_conf_t *core;
  (void)cmd; (void)conf;
  core = ngx_http_conf_get_module_loc_conf(cf, ngx_http_core_module);
  core->handler = pc_status_handler;
  return NGX_CONF_OK;
}
