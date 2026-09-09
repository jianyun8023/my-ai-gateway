-- Issue #110: persist only system-level events that do not already have a
-- domain fact table, then expose them through the unified event read model.
-- Event details are metadata-only JSON; request/response bodies and secrets
-- are forbidden by the application writer.

INSERT INTO gateway_schema_migrations (version, name)
VALUES (24, 'system_events')
ON CONFLICT (version) DO NOTHING;

UPDATE gateway_schema_metadata
SET schema_version = GREATEST(schema_version, 24),
    migration_version = GREATEST(migration_version, 24),
    updated_at = NOW()
WHERE singleton = TRUE;

CREATE TABLE IF NOT EXISTS system_events (
  id BIGSERIAL PRIMARY KEY,
  occurred_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
  category TEXT NOT NULL CHECK (category IN ('lifecycle', 'configuration', 'database', 'security')),
  event_type TEXT NOT NULL CHECK (event_type ~ '^[a-z0-9][a-z0-9._-]{0,127}$'),
  level TEXT NOT NULL CHECK (level IN ('info', 'warning', 'error')),
  subject_type TEXT NOT NULL CHECK (char_length(subject_type) BETWEEN 1 AND 64),
  subject_id TEXT CHECK (subject_id IS NULL OR char_length(subject_id) BETWEEN 1 AND 256),
  correlation_id TEXT CHECK (correlation_id IS NULL OR char_length(correlation_id) BETWEEN 1 AND 256),
  message TEXT NOT NULL CHECK (char_length(message) BETWEEN 1 AND 256),
  details JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(details) = 'object')
);

CREATE INDEX IF NOT EXISTS idx_system_events_occurred_at
  ON system_events (occurred_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_system_events_category_level
  ON system_events (category, level, occurred_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_system_events_type
  ON system_events (event_type, occurred_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_system_events_subject
  ON system_events (subject_type, subject_id, occurred_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_system_events_correlation
  ON system_events (correlation_id, occurred_at DESC, id DESC)
  WHERE correlation_id IS NOT NULL;

-- The original constraint was declared inline by migration 0011, so its
-- PostgreSQL-generated name is deterministic for this table/column pair.
ALTER TABLE retention_policies
  DROP CONSTRAINT IF EXISTS retention_policies_policy_key_check;
ALTER TABLE retention_policies
  ADD CONSTRAINT retention_policies_policy_key_check
  CHECK (policy_key IN ('usage_events', 'usage_attempts', 'audit', 'discovery', 'system_events'));

INSERT INTO retention_policies (policy_key, retention_days, enabled)
VALUES ('system_events', 365, TRUE)
ON CONFLICT (policy_key) DO NOTHING;

ALTER TABLE retention_cleanup_runs
  ADD COLUMN IF NOT EXISTS scanned_system_events BIGINT NOT NULL DEFAULT 0 CHECK (scanned_system_events >= 0),
  ADD COLUMN IF NOT EXISTS deleted_system_events BIGINT NOT NULL DEFAULT 0 CHECK (deleted_system_events >= 0);
