CREATE TABLE IF NOT EXISTS usage_events (
  id BIGSERIAL PRIMARY KEY,
  request_id TEXT NOT NULL UNIQUE,
  provider_id TEXT NOT NULL,
  account_id TEXT NOT NULL,
  model TEXT NOT NULL,
  logical_model TEXT NOT NULL,
  upstream_model_id TEXT,
  source TEXT NOT NULL DEFAULT 'unknown',
  protocol_in TEXT NOT NULL,
  protocol_upstream TEXT NOT NULL,
  mode TEXT NOT NULL,
  status_code INTEGER NOT NULL,
  success BOOLEAN NOT NULL,
  retry_count INTEGER NOT NULL DEFAULT 0,
  latency_ms BIGINT NOT NULL DEFAULT 0,
  ttft_ms BIGINT,
  input_tokens BIGINT NOT NULL DEFAULT 0,
  output_tokens BIGINT NOT NULL DEFAULT 0,
  reasoning_tokens BIGINT NOT NULL DEFAULT 0,
  cached_tokens BIGINT NOT NULL DEFAULT 0,
  total_tokens BIGINT NOT NULL DEFAULT 0,
  usage_source TEXT NOT NULL DEFAULT 'missing',
  degraded BOOLEAN NOT NULL DEFAULT FALSE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_usage_events_created_at ON usage_events (created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_model ON usage_events (model);
CREATE INDEX IF NOT EXISTS idx_usage_events_provider_account ON usage_events (provider_id, account_id);

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

CREATE TABLE IF NOT EXISTS virtual_keys (
  id BIGSERIAL PRIMARY KEY,
  name TEXT NOT NULL,
  key_prefix TEXT NOT NULL,
  key_hash TEXT NOT NULL UNIQUE,
  allowed_models JSONB NOT NULL DEFAULT '[]'::jsonb,
  enabled BOOLEAN NOT NULL DEFAULT TRUE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  last_used_at TIMESTAMPTZ,
  revoked_at TIMESTAMPTZ
);
