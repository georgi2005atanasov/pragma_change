CREATE TABLE apis (
 id uuid PRIMARY KEY, name text NOT NULL UNIQUE, host text NOT NULL UNIQUE,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE endpoints (
 id uuid PRIMARY KEY, api_id uuid NOT NULL REFERENCES apis(id), name text NOT NULL,
 method text NOT NULL, path_template text NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(), UNIQUE(api_id, method, path_template)
);
CREATE TABLE pipeline_versions (
 id uuid PRIMARY KEY, endpoint_id uuid NOT NULL REFERENCES endpoints(id),
 version integer NOT NULL CHECK(version > 0), schema_hash text NOT NULL,
 configuration jsonb NOT NULL, created_at timestamptz NOT NULL DEFAULT now(), UNIQUE(endpoint_id, version)
);
CREATE TABLE field_extractors (
 id uuid PRIMARY KEY, pipeline_version_id uuid NOT NULL REFERENCES pipeline_versions(id),
 name text NOT NULL, source jsonb NOT NULL, UNIQUE(pipeline_version_id,name)
);
CREATE TABLE partition_definitions (
 id uuid PRIMARY KEY, pipeline_version_id uuid NOT NULL REFERENCES pipeline_versions(id),
 name text NOT NULL, UNIQUE(pipeline_version_id,name)
);
CREATE TABLE partition_components (
 partition_definition_id uuid NOT NULL REFERENCES partition_definitions(id),
 field_extractor_id uuid NOT NULL REFERENCES field_extractors(id),
 position integer NOT NULL CHECK(position >= 0), PRIMARY KEY(partition_definition_id,position),
 UNIQUE(partition_definition_id,field_extractor_id)
);
CREATE TABLE window_definitions (
 id uuid PRIMARY KEY, pipeline_version_id uuid NOT NULL REFERENCES pipeline_versions(id),
 name text NOT NULL, duration_ms bigint NOT NULL CHECK(duration_ms > 0), UNIQUE(pipeline_version_id,name)
);
CREATE TABLE feature_definitions (
 id uuid PRIMARY KEY, pipeline_version_id uuid NOT NULL REFERENCES pipeline_versions(id),
 name text NOT NULL, position integer NOT NULL, operation text NOT NULL,
 field_extractor_id uuid REFERENCES field_extractors(id),
 partition_definition_id uuid REFERENCES partition_definitions(id), window_definition_id uuid REFERENCES window_definitions(id),
 UNIQUE(pipeline_version_id,name), UNIQUE(pipeline_version_id,position)
);
CREATE TABLE datasets (
 id uuid PRIMARY KEY, pipeline_version_id uuid NOT NULL REFERENCES pipeline_versions(id), name text NOT NULL,
 format text NOT NULL CHECK(format IN ('request_events','feature_vectors')),
 schema_hash text NOT NULL, content_hash text NOT NULL, artifact_path text NOT NULL,
 row_count integer NOT NULL CHECK(row_count > 0), validation_report jsonb NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE training_jobs (
 id uuid PRIMARY KEY, dataset_id uuid NOT NULL REFERENCES datasets(id),
 status text NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','running','succeeded','failed')),
 parameters jsonb NOT NULL, attempts integer NOT NULL DEFAULT 0, error text,
 lease_until timestamptz, next_run_at timestamptz NOT NULL DEFAULT now(),
 created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX training_jobs_pending ON training_jobs(next_run_at) WHERE status IN ('queued','running');
CREATE TABLE model_versions (
 id uuid PRIMARY KEY, training_job_id uuid NOT NULL UNIQUE REFERENCES training_jobs(id),
 pipeline_version_id uuid NOT NULL REFERENCES pipeline_versions(id), schema_hash text NOT NULL,
 artifact_path text NOT NULL, content_hash text NOT NULL, evaluation jsonb NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE releases (
 id uuid PRIMARY KEY, name text NOT NULL, configuration jsonb NOT NULL,
 content_hash text NOT NULL, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE release_models (
 release_id uuid NOT NULL REFERENCES releases(id), model_version_id uuid NOT NULL REFERENCES model_versions(id),
 PRIMARY KEY(release_id, model_version_id)
);
CREATE TABLE admin_users (
 id uuid PRIMARY KEY, username text NOT NULL UNIQUE, password_hash text NOT NULL,
 failed_login_count integer NOT NULL DEFAULT 0, locked_until timestamptz,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE sessions (
 token_hash text PRIMARY KEY, admin_user_id uuid NOT NULL REFERENCES admin_users(id),
 csrf_hash text NOT NULL, expires_at timestamptz NOT NULL, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE audit_events (
 id uuid PRIMARY KEY, actor text NOT NULL, action text NOT NULL, entity_id uuid,
 details jsonb NOT NULL DEFAULT '{}', created_at timestamptz NOT NULL DEFAULT now()
);
CREATE FUNCTION prevent_immutable_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'versioned records are immutable'; END $$;
CREATE TRIGGER immutable_pipeline BEFORE UPDATE OR DELETE ON pipeline_versions FOR EACH ROW EXECUTE FUNCTION prevent_immutable_change();
CREATE TRIGGER immutable_model BEFORE UPDATE OR DELETE ON model_versions FOR EACH ROW EXECUTE FUNCTION prevent_immutable_change();
CREATE TRIGGER immutable_release BEFORE UPDATE OR DELETE ON releases FOR EACH ROW EXECUTE FUNCTION prevent_immutable_change();
