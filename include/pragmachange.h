#ifndef PRAGMACHANGE_H
#define PRAGMACHANGE_H
#include <stddef.h>
#include <stdint.h>
#define PC_ABI_VERSION 3
typedef struct {
  const uint8_t *data;
  size_t len;
} PcSlice;
typedef struct {
  PcSlice name, value;
} PcHeader;
typedef struct {
  uint64_t timestamp_ms;
  PcSlice method, path, host, query, client_ip, body;
  const PcHeader *headers;
  size_t header_count;
  uint64_t body_size;
  uint32_t input_error;
  uint32_t has_multipart_metrics;
  uint32_t multipart_parts, multipart_file_parts;
  uint32_t multipart_max_part_headers, multipart_max_field_nesting;
  uint32_t multipart_total_header_bytes, multipart_empty_file_parts;
} PcRequest;
typedef struct {
  uint32_t action, has_multipart_metrics;
  uint8_t reason[128];
  uint32_t multipart_parts, multipart_file_parts;
  uint32_t multipart_max_part_headers, multipart_max_field_nesting;
  uint32_t multipart_total_header_bytes, multipart_empty_file_parts;
} PcGuardDecision;
typedef struct {
  uint32_t action, has_score;
  double score;
  uint8_t reason[128], pipeline_version_id[37], model_version_id[37];
} PcDecision;
typedef void PcRuntime;
typedef void PcPrepared;
typedef void PcGuardStream;
typedef int32_t (*PcGet)(void *, PcSlice, PcSlice *);
typedef int32_t (*PcPut)(void *, PcSlice, PcSlice, size_t);
PcRuntime *pc_runtime_new(PcSlice, PcSlice, uint8_t error[256]);
int32_t pc_route(const PcRuntime *, PcSlice, PcSlice, PcSlice, uint32_t *,
                 size_t *);
int32_t pc_route_full_body(const PcRuntime *, int32_t);
int32_t pc_guard_policy(const PcRuntime *, PcSlice, size_t *);
int32_t pc_guard_decide(const PcRuntime *, int32_t, PcSlice, PcSlice, PcGuardDecision *);
PcGuardStream *pc_guard_stream_new(const PcRuntime *, int32_t, PcSlice);
int32_t pc_guard_stream_feed(PcGuardStream *, PcSlice);
int32_t pc_guard_stream_finish(PcGuardStream *, PcGuardDecision *);
void pc_guard_stream_free(PcGuardStream *);
int32_t pc_ingress_policy_count(const PcRuntime *);
PcPrepared *pc_features(const PcRuntime *, int32_t, const PcRequest *, PcGet,
                        PcPut, void *);
int32_t pc_decide(const PcRuntime *, const PcPrepared *, PcDecision *);
void pc_prepared_free(PcPrepared *);
void pc_runtime_free(PcRuntime *);
int32_t pc_release_id(const PcRuntime *, uint8_t out[37]);
#endif
