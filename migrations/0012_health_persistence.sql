-- Issue #52: persist account health independently from account configuration updates.
-- `updated_at` describes control-plane edits; health timestamps below are the
-- observation clock used for cooldown expiry and stale reporting.
ALTER TABLE accounts
  ADD COLUMN IF NOT EXISTS health_source TEXT NOT NULL DEFAULT 'unknown',
  ADD COLUMN IF NOT EXISTS health_updated_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS consecutive_failures INTEGER NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS last_probe_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS last_probe_status TEXT,
  ADD COLUMN IF NOT EXISTS last_probe_error TEXT;

UPDATE accounts
SET health_updated_at = COALESCE(health_updated_at, last_success_at, updated_at, created_at),
    consecutive_failures = GREATEST(consecutive_failures, 0),
    health_source = CASE
      WHEN health_source IS NULL OR btrim(health_source) = '' THEN 'unknown'
      ELSE health_source
    END
WHERE health_updated_at IS NULL
   OR consecutive_failures < 0
   OR health_source IS NULL
   OR btrim(health_source) = '';

ALTER TABLE accounts
  ALTER COLUMN health_updated_at SET DEFAULT NOW();

UPDATE accounts
SET health_status = 'unknown'
WHERE health_status IS NULL
   OR health_status NOT IN ('unknown', 'healthy', 'cooling_down', 'unhealthy', 'disabled');

DO $$ BEGIN
  ALTER TABLE accounts
    ADD CONSTRAINT accounts_health_source_check
    CHECK (health_source IN ('unknown', 'passive', 'probe', 'manual', 'startup'));
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE accounts
    ADD CONSTRAINT accounts_consecutive_failures_check
    CHECK (consecutive_failures >= 0);
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE accounts
    ADD CONSTRAINT accounts_health_status_check
    CHECK (health_status IN ('unknown', 'healthy', 'cooling_down', 'unhealthy', 'stale', 'disabled'));
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;

CREATE INDEX IF NOT EXISTS idx_accounts_health_runtime
  ON accounts (enabled, health_status, cooldown_until, health_updated_at);

-- Keep an append-only, body-free transition history for operations and
-- debugging. Retention/cleanup is owned by the data-retention work; the
-- account row remains the current source of truth and routing does not read
-- this history table.
CREATE TABLE IF NOT EXISTS account_health_events (
  id BIGSERIAL PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  status TEXT NOT NULL CHECK (status IN ('unknown', 'healthy', 'cooling_down', 'unhealthy', 'stale', 'disabled')),
  source TEXT NOT NULL CHECK (source IN ('unknown', 'passive', 'probe', 'manual', 'startup')),
  observed_at TIMESTAMPTZ NOT NULL,
  cooldown_until TIMESTAMPTZ,
  consecutive_failures INTEGER NOT NULL CHECK (consecutive_failures >= 0),
  error_code TEXT,
  error_message TEXT,
  connection_test_id BIGINT REFERENCES source_connection_tests(id) ON DELETE SET NULL,
  latency_ms BIGINT CHECK (latency_ms IS NULL OR latency_ms >= 0),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_account_health_events_latest
  ON account_health_events (account_id, observed_at DESC, id DESC);
