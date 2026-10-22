CREATE TABLE behavior_apps (
 id uuid PRIMARY KEY, name text NOT NULL, token_hash text NOT NULL UNIQUE,
 policy jsonb NOT NULL, active_model uuid, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE behavior_events (
 application_id uuid NOT NULL REFERENCES behavior_apps(id), id uuid NOT NULL,
 partition_key text NOT NULL, content_hash text NOT NULL, event jsonb NOT NULL,
 timestamp_ms bigint NOT NULL, quarantined boolean NOT NULL DEFAULT false,
 received_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY(application_id,id)
);
CREATE INDEX behavior_events_snapshot ON behavior_events(application_id,timestamp_ms,id);
CREATE TABLE behavior_state (
 application_id uuid NOT NULL REFERENCES behavior_apps(id), partition_key text NOT NULL,
 history jsonb NOT NULL, updated_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY(application_id,partition_key)
);
CREATE TABLE behavior_decisions (
 application_id uuid NOT NULL REFERENCES behavior_apps(id), event_id uuid NOT NULL,
 content_hash text NOT NULL, context jsonb NOT NULL, decision jsonb NOT NULL, created_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(application_id,event_id)
);
CREATE TABLE behavior_snapshots (
 id uuid PRIMARY KEY, application_id uuid NOT NULL REFERENCES behavior_apps(id),
 content_hash text NOT NULL, events jsonb NOT NULL, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE behavior_training_jobs (
 id uuid PRIMARY KEY, snapshot_id uuid NOT NULL REFERENCES behavior_snapshots(id),
 status text NOT NULL DEFAULT 'queued', error text, lease_until timestamptz,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE behavior_models (
 id uuid PRIMARY KEY, application_id uuid NOT NULL REFERENCES behavior_apps(id),
 snapshot_id uuid NOT NULL REFERENCES behavior_snapshots(id), model jsonb NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE behavior_audit (
 id bigserial PRIMARY KEY, application_id uuid, action text NOT NULL, details jsonb NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX behavior_one_outcome_per_attempt ON behavior_events(application_id,(event->>'correlation_id')) WHERE event->>'outcome' IN ('succeeded','failed') AND event->>'correlation_id' IS NOT NULL;
