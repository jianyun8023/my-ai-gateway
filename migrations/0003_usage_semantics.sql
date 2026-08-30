ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS logical_model TEXT;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS upstream_model_id TEXT;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS source TEXT NOT NULL DEFAULT 'unknown';

UPDATE usage_events SET logical_model = model WHERE logical_model IS NULL;
ALTER TABLE usage_events ALTER COLUMN logical_model SET DEFAULT '';

CREATE TABLE IF NOT EXISTS usage_event_attempts (
  id BIGSERIAL PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES usage_events(request_id) ON DELETE CASCADE,
  attempt_no INTEGER NOT NULL,
  provider_id TEXT NOT NULL,
  account_id TEXT NOT NULL,
  upstream_model_id TEXT,
  status_code INTEGER NOT NULL,
  success BOOLEAN NOT NULL,
  latency_ms BIGINT NOT NULL DEFAULT 0,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE (request_id, attempt_no)
);
CREATE INDEX IF NOT EXISTS idx_usage_event_attempts_request ON usage_event_attempts (request_id, attempt_no);
CREATE INDEX IF NOT EXISTS idx_usage_event_attempts_created_at ON usage_event_attempts (created_at DESC);
