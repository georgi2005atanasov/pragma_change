-- Preserve the original recording identity separately from pseudonymized snapshot content.
ALTER TABLE behavior_snapshots ADD COLUMN source_hash text;
ALTER TABLE behavior_snapshots ADD COLUMN source_manifest jsonb;
