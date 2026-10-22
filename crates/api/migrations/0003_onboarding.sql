CREATE TABLE ai_providers (
 provider text PRIMARY KEY CHECK(provider='claude'), model text NOT NULL,
 encrypted_key text NOT NULL, daily_requests integer NOT NULL CHECK(daily_requests BETWEEN 1 AND 100),
 updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE onboarding_jobs (
 id uuid PRIMARY KEY, application_id uuid, description text NOT NULL,
 status text NOT NULL DEFAULT 'queued', draft jsonb, report jsonb NOT NULL DEFAULT '{}',
 error text, lease_until timestamptz, created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE onboarding_steps (
 job_id uuid NOT NULL REFERENCES onboarding_jobs(id), step text NOT NULL,
 result jsonb NOT NULL, PRIMARY KEY(job_id,step)
);
