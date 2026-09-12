-- Model-level request policy used by the ordered upstream-line editor.
INSERT INTO gateway_schema_migrations (version, name)
VALUES (25, 'logical_model_request_settings')
ON CONFLICT (version) DO NOTHING;

UPDATE gateway_schema_metadata
SET schema_version = GREATEST(schema_version, 25),
    migration_version = GREATEST(migration_version, 25),
    updated_at = NOW()
WHERE singleton = TRUE;

ALTER TABLE logical_models
  ADD COLUMN IF NOT EXISTS request_timeout_ms BIGINT CHECK (request_timeout_ms > 0),
  ADD COLUMN IF NOT EXISTS max_retries INTEGER CHECK (max_retries >= 0);
