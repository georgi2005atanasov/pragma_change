#!/usr/bin/env python3
"""Generate the checked-in management contract; run when HTTP contracts change."""
import json
from pathlib import Path


def obj(properties, required=None, **extra):
    return {"type": "object", "properties": properties, "required": required if required is not None else list(properties), "additionalProperties": False, **extra}


def ref(name):
    return {"$ref": f"#/components/schemas/{name}"}


def array(item):
    return {"type": "array", "items": item}


string = {"type": "string"}
number = {"type": "number"}
integer = {"type": "integer"}
uuid = {"type": "string", "format": "uuid"}
nullable_string = {"type": ["string", "null"]}
schemas = {
    "Error": obj({"error": string}),
    "Identifier": obj({"id": uuid}),
    "Thresholds": obj({"check": {"type": "number", "minimum": 0, "maximum": 1}, "block": {"type": "number", "minimum": 0, "maximum": 1}}, description="Require check < block. Anomaly scores are not attack probabilities."),
    "Source": {"oneOf": [obj({"kind": {"const": kind}, **({field: string} if field else {})}) for kind, field in [("header", "name"), ("json_pointer", "pointer"), ("query_parameter", "name"), ("path_parameter", "name"), ("client_ip", None)]]},
    "Extractor": obj({"name": string, "source": ref("Source")}),
    "Partition": obj({"name": string, "components": array(string)}),
    "Window": obj({"name": string, "duration_ms": {"type": "integer", "minimum": 1, "maximum": 86400000}}),
    "Feature": obj({"name": string, "operation": {"enum": ["count", "rate", "sum", "mean", "min", "max", "stddev", "distinct_count", "interarrival_mean", "interarrival_stddev", "current_value", "body_size", "multipart_part_count", "multipart_file_count", "multipart_max_part_headers", "multipart_max_field_nesting", "multipart_total_header_bytes", "multipart_empty_file_count"]}, "source": nullable_string, "partition": nullable_string, "window": nullable_string}),
    "Limits": obj({"max_body_bytes": integer, "max_partitions": integer, "max_events_per_partition": integer, "max_partition_bytes": integer}),
    "RequestEvent": obj({"timestamp_ms": integer, "endpoint_id": uuid, "method": string, "path": string, "headers": {"type": "object", "additionalProperties": string}, "query": {"type": "object", "additionalProperties": string}, "client_ip": string, "body": {}, "body_size": integer, "multipart": {"type": ["object", "null"], "properties": {"parts": integer, "file_parts": integer, "max_part_headers": integer, "max_field_nesting": integer, "total_header_bytes": integer, "empty_file_parts": integer}}}, ["timestamp_ms", "endpoint_id", "method", "path"]),
    "TrainingParameters": obj({"n_estimators": {"type": "integer", "minimum": 1, "maximum": 512, "default": 100}, "max_samples": {"type": "integer", "minimum": 2, "maximum": 65536, "default": 256}, "seed": {"type": "integer", "minimum": 0, "maximum": 4294967295, "default": 42}}, []),
    "Decision": obj({"action": {"enum": ["allow", "check", "block"]}, "risk_score": {"type": ["number", "null"]}, "reason": string, "pipeline_version_id": uuid, "model_version_id": {"type": ["string", "null"], "format": "uuid"}}),
}
schemas["Pipeline"] = obj({"id": uuid, "endpoint_id": uuid, "api_host": string, "method": string, "path_template": string, "extractors": array(ref("Extractor")), "partitions": array(ref("Partition")), "windows": array(ref("Window")), "features": array(ref("Feature")), "limits": ref("Limits")}, description="POST assigns a fresh id and derives host/method/path from endpoint_id. Feature order is significant. Versions are immutable.")
schemas["DatasetImport"] = obj({"pipeline_version_id": uuid, "name": string, "format": {"enum": ["request_events", "feature_vectors"]}, "content": string, "schema_hash": string}, description="Maximum request size 16 MiB; up to 100,000 rows. JSONL event rows or CSV with exact named columns. At least 20 complete vectors for import.")
schemas["RecordingManifest"] = json.loads((Path(__file__).resolve().parent.parent / "docs/recording.schema.json").read_text())
schemas["RecordingManifest"].pop("$schema", None)
schemas["DatasetImport"]["properties"]["recording_manifest"] = {"oneOf": [ref("RecordingManifest"), {"type": "null"}]}
paths = {}


def route(path, method, summary, request=None, response=None, public=False, binary=False):
    operation = {"summary": summary, "operationId": method + "_" + path.strip("/").replace("/", "_").replace("{", "").replace("}", "").replace("-", "_").replace(".", "_"), "responses": {"200": {"description": "Success", "content": {"application/gzip" if binary else "application/json": {"schema": response or {"type": "object"}}}}, **{str(code): {"description": description, "content": {"application/json": {"schema": ref("Error")}}} for code, description in [(401, "Session required"), (403, "CSRF validation failed"), (404, "Not found"), (409, "Resource conflict"), (422, "Validation error")]}}}
    if public:
        operation["security"] = []
    if "{id}" in path:
        operation.setdefault("parameters", []).append({"in": "path", "name": "id", "required": True, "schema": uuid})
    if method == "post" and not public:
        operation.setdefault("parameters", []).append({"in": "header", "name": "X-CSRF-Token", "required": True, "schema": string})
    if request is not None:
        operation["requestBody"] = {"required": True, "content": {"application/json": {"schema": request}}}
    paths.setdefault(path, {})[method] = operation


route("/login", "post", "Create a 12-hour administrator session", obj({"username": string, "password": string}), obj({"token": string, "csrf_token": string, "expires_in": integer}), public=True)
route("/logout", "post", "Revoke the current session", obj({}))
route("/overview", "get", "Workspace counts")
for table in ["apis", "endpoints", "pipelines", "datasets", "training-jobs", "models", "releases", "audit-events"]:
    route("/" + table, "get", "List latest 200 " + table + " (newest first)", response=array({"type": "object"}))
route("/apis", "post", "Register an API host", obj({"name": string, "host": string}), ref("Identifier"))
route("/endpoints", "post", "Create endpoint; reject overlapping routes", obj({"api_id": uuid, "name": string, "method": string, "path_template": string}), ref("Identifier"))
route("/pipelines", "post", "Validate and save an immutable pipeline version", ref("Pipeline"))
route("/pipelines/{id}/template", "get", "Generate CSV, JSONL and schema-manifest examples")
route("/pipelines/{id}/replay", "post", "Replay events in isolated fresh window state", obj({"events": array(ref("RequestEvent")), "model_version_id": {"type": ["string", "null"], "format": "uuid"}, "thresholds": {"oneOf": [ref("Thresholds"), {"type": "null"}]}}, ["events"]))
route("/datasets/preview", "post", "Validate without saving; return row errors and feature preview", ref("DatasetImport"))
route("/datasets", "post", "Validate and import dataset with at least 20 usable rows", ref("DatasetImport"))
route("/training-jobs", "post", "Queue a durable training job", obj({"dataset_id": uuid, "parameters": ref("TrainingParameters")}, ["dataset_id"]), ref("Identifier"))
route("/training-jobs/{id}/retry", "post", "Retry a failed training job", obj({}))
route("/models/{id}", "get", "Model metadata with dataset, training job, pipeline, endpoint and API provenance", response={"type": "object", "required": ["id", "dataset", "training_job", "pipeline", "endpoint", "api"], "properties": {"id": uuid, "dataset": {"type": "object"}, "training_job": {"type": "object"}, "pipeline": {"type": "object"}, "endpoint": {"type": "object"}, "api": {"type": "object"}}, "additionalProperties": True})
route("/models/{id}/score", "post", "Score a vector with explicit thresholds", obj({"vector": array(number), "thresholds": ref("Thresholds")}), ref("Decision"))
route("/releases", "post", "Validate and create an immutable release", obj({"name": string, "models": array(obj({"model_version_id": uuid, "thresholds": ref("Thresholds")}))}), ref("Identifier"))
route("/releases/{id}/download", "get", "Export verified release archive", response={"type": "string", "format": "binary"}, binary=True)
paths["/releases/{id}/download"]["get"]["parameters"].append({"in": "query", "name": "target", "required": False, "schema": {"type": "string", "enum": ["nginx", "apache"], "default": "nginx"}, "description": "Select proxy-specific deployment instructions. Model release JSON is unchanged. Unsupported or duplicate targets are rejected."})
paths["/releases/{id}/download"]["get"]["responses"]["400"] = {"description": "Invalid download target or query parameters"}
route("/openapi.json", "get", "Read this API contract")
doc = {"openapi": "3.1.0", "info": {"title": "PragmaChange management API", "version": "0.1.0", "description": "Single-administrator control plane. Production inference uses the native C ABI, never this HTTP API. JSON fields and PostgreSQL identifiers use snake_case."}, "servers": [{"url": "/api/v1"}], "security": [{"session": []}], "paths": paths, "components": {"securitySchemes": {"session": {"type": "http", "scheme": "bearer"}}, "schemas": schemas}}
(Path(__file__).resolve().parent.parent / "docs/openapi.json").write_text(json.dumps(doc, indent=2) + "\n")
