CREATE TABLE managed_servers (
 id uuid PRIMARY KEY, name text NOT NULL, address text NOT NULL,
 port integer NOT NULL CHECK(port BETWEEN 1 AND 65535), username text NOT NULL,
 host_key text NOT NULL, encrypted_key text NOT NULL,
 discovery jsonb, desired_release uuid REFERENCES releases(id), observed_release uuid,
 verified_at timestamptz, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE remote_jobs (
 id uuid PRIMARY KEY, server_id uuid NOT NULL REFERENCES managed_servers(id),
 operation text NOT NULL CHECK(operation IN ('inspect','preview','module_preview','stage_module','deploy','rollback','reconcile')),
 request jsonb NOT NULL, status text NOT NULL DEFAULT 'queued'
 CHECK(status IN ('queued','running','succeeded','failed','unknown','skipped')),
 result jsonb, batch_id uuid NOT NULL, ordinal integer NOT NULL DEFAULT 0,
 lease_until timestamptz, created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(batch_id, ordinal)
);
CREATE INDEX remote_jobs_pending ON remote_jobs(created_at) WHERE status IN ('queued','running');
