-- Issue #129: do not open the account circuit after one transient retryable
-- failure. Track the start of the current failure window so the runtime can
-- require a threshold before applying exponential cooldown.

ALTER TABLE accounts
  ADD COLUMN IF NOT EXISTS failure_window_started_at TIMESTAMPTZ;

-- Existing failure counters predate window semantics and may have accumulated
-- over many hours. Reset them rather than carrying an unbounded historical
-- streak into the new threshold policy. The migration scripts are replayed on
-- every startup, so this state repair must run only when version 22 is first
-- registered.
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM gateway_schema_migrations WHERE version = 22
  ) THEN
    INSERT INTO gateway_schema_migrations (version, name)
    VALUES (22, 'health_failure_window');

    UPDATE gateway_schema_metadata
    SET schema_version = GREATEST(schema_version, 22),
        migration_version = GREATEST(migration_version, 22),
        updated_at = NOW()
    WHERE singleton = TRUE;

    UPDATE accounts
    SET health_status = CASE
          WHEN enabled THEN 'unknown'
          ELSE 'disabled'
        END,
        cooldown_until = NULL,
        consecutive_failures = 0,
        failure_window_started_at = NULL,
        last_error = NULL,
        health_source = 'unknown',
        health_updated_at = NOW()
    WHERE consecutive_failures > 0
       OR cooldown_until IS NOT NULL;
  END IF;
END $$;
