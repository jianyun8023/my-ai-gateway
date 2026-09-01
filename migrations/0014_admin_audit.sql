-- Issue #48: extend the operational audit table with a common Admin-write
-- contract.  The legacy operation_id/action/status/details columns remain in
-- place for the retention and backup run history introduced by Issue #53.
-- New fields are deliberately metadata-only: request bodies and credentials
-- are never persisted here.

INSERT INTO gateway_schema_migrations (version, name)
VALUES (14, 'admin_audit')
ON CONFLICT (version) DO NOTHING;

ALTER TABLE audit_logs
  ADD COLUMN IF NOT EXISTS request_id TEXT,
  ADD COLUMN IF NOT EXISTS resource_type TEXT,
  ADD COLUMN IF NOT EXISTS resource_id TEXT,
  ADD COLUMN IF NOT EXISTS resource TEXT,
  ADD COLUMN IF NOT EXISTS result TEXT NOT NULL DEFAULT 'success',
  ADD COLUMN IF NOT EXISTS diff JSONB NOT NULL DEFAULT '{}'::jsonb;

-- Rows written by the retention/backup implementation predate the common
-- result field.  Keep their historical status while making the new column
-- useful to filtered readers.
UPDATE audit_logs
SET result = CASE
  WHEN status = 'failed' THEN 'failure'
  WHEN status = 'cancelled' THEN 'failure'
  ELSE 'success'
END
WHERE result IS NULL OR result = 'success' AND status IN ('failed', 'cancelled');

DO $$ BEGIN
  ALTER TABLE audit_logs
    ADD CONSTRAINT audit_logs_result_check
    CHECK (result IN ('success', 'failure', 'conflict', 'rollback'));
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE audit_logs
    ADD CONSTRAINT audit_logs_diff_object_check
    CHECK (jsonb_typeof(diff) = 'object');
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;

CREATE INDEX IF NOT EXISTS idx_audit_logs_request_id
  ON audit_logs (request_id, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_actor_created
  ON audit_logs (actor, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_action_created
  ON audit_logs (action, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_resource_created
  ON audit_logs (resource_type, resource_id, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_result_created
  ON audit_logs (result, created_at DESC, id DESC);

UPDATE gateway_schema_metadata
SET schema_version = GREATEST(schema_version, 14),
    migration_version = GREATEST(migration_version, 14),
    updated_at = NOW()
WHERE singleton = TRUE;
