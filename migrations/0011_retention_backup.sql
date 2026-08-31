-- Issue #53: operational metadata, independent retention policies, and
-- resumable cleanup/backup audit records.  All timestamps are TIMESTAMPTZ so
-- PostgreSQL stores and compares them in UTC.

CREATE TABLE IF NOT EXISTS gateway_schema_migrations (
  version INTEGER PRIMARY KEY CHECK (version > 0),
  name TEXT NOT NULL,
  applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS gateway_schema_metadata (
  singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
  schema_version INTEGER NOT NULL CHECK (schema_version > 0),
  migration_version INTEGER NOT NULL CHECK (migration_version > 0),
  application_version TEXT NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO gateway_schema_migrations (version, name)
VALUES
  (1, 'init'),
  (2, 'control_plane'),
  (3, 'model_catalog'),
  (4, 'usage_query_contract'),
  (5, 'usage_event_fields'),
  (6, 'control_plane_crud'),
  (7, 'db_first_runtime'),
  (8, 'provider_discovery'),
  (9, 'usage_source_dimensions'),
  (10, 'usage_provider_attribution'),
  (11, 'retention_backup')
ON CONFLICT (version) DO NOTHING;

INSERT INTO gateway_schema_metadata (
  singleton, schema_version, migration_version, application_version
)
VALUES (TRUE, 11, 11, '0.1.0')
ON CONFLICT (singleton) DO UPDATE SET
  schema_version = GREATEST(gateway_schema_metadata.schema_version, EXCLUDED.schema_version),
  migration_version = GREATEST(gateway_schema_metadata.migration_version, EXCLUDED.migration_version),
  application_version = EXCLUDED.application_version,
  updated_at = NOW();

CREATE TABLE IF NOT EXISTS retention_policies (
  policy_key TEXT PRIMARY KEY CHECK (policy_key IN ('usage_events', 'usage_attempts', 'audit', 'discovery')),
  retention_days INTEGER NOT NULL CHECK (retention_days >= 0 AND retention_days <= 36500),
  enabled BOOLEAN NOT NULL DEFAULT TRUE,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO retention_policies (policy_key, retention_days, enabled)
VALUES
  ('usage_events', 90, TRUE),
  ('usage_attempts', 90, TRUE),
  ('audit', 365, TRUE),
  ('discovery', 365, TRUE)
ON CONFLICT (policy_key) DO NOTHING;

CREATE TABLE IF NOT EXISTS audit_logs (
  id BIGSERIAL PRIMARY KEY,
  operation_id TEXT NOT NULL,
  action TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('started', 'progress', 'succeeded', 'failed', 'cancel_requested', 'cancelled')),
  actor TEXT NOT NULL DEFAULT 'system',
  details JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(details) = 'object'),
  error_code TEXT,
  error_message TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  completed_at TIMESTAMPTZ,
  CHECK (completed_at IS NULL OR completed_at >= created_at)
);

CREATE INDEX IF NOT EXISTS idx_audit_logs_operation_created
  ON audit_logs (operation_id, created_at, id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_created_at
  ON audit_logs (created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_source_connection_tests_retention
  ON source_connection_tests (tested_at ASC, id ASC);
CREATE INDEX IF NOT EXISTS idx_source_discovery_runs_retention
  ON source_discovery_runs (completed_at ASC, id ASC);
CREATE INDEX IF NOT EXISTS idx_usage_event_attempts_retention
  ON usage_event_attempts (created_at ASC, id ASC);
CREATE INDEX IF NOT EXISTS idx_usage_events_retention
  ON usage_events (created_at ASC, id ASC);

CREATE TABLE IF NOT EXISTS retention_cleanup_runs (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'failed', 'cancel_requested', 'cancelled')),
  dry_run BOOLEAN NOT NULL DEFAULT FALSE,
  batch_size INTEGER NOT NULL CHECK (batch_size > 0 AND batch_size <= 10000),
  max_batches INTEGER NOT NULL DEFAULT 1000 CHECK (max_batches > 0 AND max_batches <= 100000),
  requested_by TEXT NOT NULL DEFAULT 'admin_api',
  policy_snapshot JSONB NOT NULL CHECK (jsonb_typeof(policy_snapshot) = 'object'),
  cutoff_snapshot JSONB NOT NULL CHECK (jsonb_typeof(cutoff_snapshot) = 'object'),
  scanned_usage_events BIGINT NOT NULL DEFAULT 0 CHECK (scanned_usage_events >= 0),
  deleted_usage_events BIGINT NOT NULL DEFAULT 0 CHECK (deleted_usage_events >= 0),
  scanned_usage_attempts BIGINT NOT NULL DEFAULT 0 CHECK (scanned_usage_attempts >= 0),
  deleted_usage_attempts BIGINT NOT NULL DEFAULT 0 CHECK (deleted_usage_attempts >= 0),
  scanned_audit BIGINT NOT NULL DEFAULT 0 CHECK (scanned_audit >= 0),
  deleted_audit BIGINT NOT NULL DEFAULT 0 CHECK (deleted_audit >= 0),
  scanned_discovery BIGINT NOT NULL DEFAULT 0 CHECK (scanned_discovery >= 0),
  deleted_discovery BIGINT NOT NULL DEFAULT 0 CHECK (deleted_discovery >= 0),
  batches_completed INTEGER NOT NULL DEFAULT 0 CHECK (batches_completed >= 0),
  progress JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(progress) = 'object'),
  last_error_code TEXT,
  last_error_message TEXT,
  cancel_requested BOOLEAN NOT NULL DEFAULT FALSE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  finished_at TIMESTAMPTZ,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CHECK (finished_at IS NULL OR finished_at >= started_at)
);

ALTER TABLE retention_cleanup_runs
  ADD COLUMN IF NOT EXISTS progress JSONB NOT NULL DEFAULT '{}'::jsonb;

CREATE INDEX IF NOT EXISTS idx_retention_cleanup_runs_status_updated
  ON retention_cleanup_runs (status, updated_at DESC);

CREATE TABLE IF NOT EXISTS backup_runs (
  id TEXT PRIMARY KEY,
  operation TEXT NOT NULL CHECK (operation IN ('control_plane_export', 'postgres_backup', 'restore')),
  status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed', 'cancelled')),
  requested_by TEXT NOT NULL DEFAULT 'admin_api',
  schema_version INTEGER NOT NULL CHECK (schema_version > 0),
  migration_version INTEGER NOT NULL CHECK (migration_version > 0),
  format TEXT NOT NULL,
  checksum TEXT,
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(metadata) = 'object'),
  error_code TEXT,
  error_message TEXT,
  started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  completed_at TIMESTAMPTZ,
  CHECK (completed_at IS NULL OR completed_at >= started_at)
);

CREATE INDEX IF NOT EXISTS idx_backup_runs_started_at
  ON backup_runs (started_at DESC, id DESC);
