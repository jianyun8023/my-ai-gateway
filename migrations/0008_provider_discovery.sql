-- Provider preset connection tests and model-discovery audit history.
-- Keep run-level snapshots separate from source_models: source_models stores the
-- current per-model state, while these rows preserve what an upstream returned
-- (or the redacted reason it could not be read) for later diff reproduction.

CREATE TABLE IF NOT EXISTS source_connection_tests (
  id BIGSERIAL PRIMARY KEY,
  source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  account_id TEXT REFERENCES accounts(id) ON DELETE SET NULL,
  protocol gateway_protocol NOT NULL,
  upstream_protocol gateway_protocol NOT NULL,
  mode source_protocol_mode NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('succeeded', 'failed')),
  http_status INTEGER CHECK (http_status BETWEEN 100 AND 599),
  latency_ms BIGINT NOT NULL CHECK (latency_ms >= 0),
  error_code TEXT,
  error_message TEXT,
  requested_by TEXT NOT NULL DEFAULT 'admin_api',
  tested_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CHECK (
    (status = 'succeeded' AND http_status BETWEEN 200 AND 299
      AND error_code IS NULL AND error_message IS NULL)
    OR
    (status = 'failed' AND error_code IS NOT NULL AND error_message IS NOT NULL)
  )
);

CREATE INDEX IF NOT EXISTS idx_source_connection_tests_latest
  ON source_connection_tests (source_id, protocol, tested_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS source_discovery_runs (
  id BIGSERIAL PRIMARY KEY,
  source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  account_id TEXT REFERENCES accounts(id) ON DELETE SET NULL,
  provider_preset_id TEXT NOT NULL,
  provider_preset_version INTEGER NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('succeeded', 'failed', 'unsupported')),
  raw_snapshot JSONB,
  diff JSONB NOT NULL DEFAULT '{"added":[],"changed":[],"missing":[]}'::jsonb,
  discovered_model_count INTEGER NOT NULL DEFAULT 0 CHECK (discovered_model_count >= 0),
  http_status INTEGER CHECK (http_status BETWEEN 100 AND 599),
  latency_ms BIGINT NOT NULL CHECK (latency_ms >= 0),
  error_code TEXT,
  error_message TEXT,
  requested_by TEXT NOT NULL DEFAULT 'admin_api',
  started_at TIMESTAMPTZ NOT NULL,
  completed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  FOREIGN KEY (provider_preset_id, provider_preset_version)
    REFERENCES provider_presets(id, version),
  CHECK (completed_at >= started_at),
  CHECK (
    (status = 'succeeded' AND raw_snapshot IS NOT NULL
      AND error_code IS NULL AND error_message IS NULL)
    OR
    (status IN ('failed', 'unsupported') AND raw_snapshot IS NULL
      AND error_code IS NOT NULL AND error_message IS NOT NULL)
  )
);

CREATE INDEX IF NOT EXISTS idx_source_discovery_runs_latest
  ON source_discovery_runs (source_id, completed_at DESC, id DESC);
