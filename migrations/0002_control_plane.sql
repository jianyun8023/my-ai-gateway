CREATE TABLE IF NOT EXISTS providers (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  enabled BOOLEAN NOT NULL DEFAULT TRUE,
  capabilities JSONB NOT NULL DEFAULT '{}'::jsonb,
  endpoints JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS accounts (
  id TEXT PRIMARY KEY,
  provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
  display_name TEXT NOT NULL,
  credential_ciphertext TEXT,
  enabled BOOLEAN NOT NULL DEFAULT TRUE,
  weight INTEGER NOT NULL DEFAULT 100,
  health_status TEXT NOT NULL DEFAULT 'unknown',
  cooldown_until TIMESTAMPTZ,
  last_error TEXT,
  last_success_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS routes (
  id TEXT PRIMARY KEY,
  model_pattern TEXT NOT NULL,
  provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
  protocols JSONB NOT NULL DEFAULT '[]'::jsonb,
  primary_account_id TEXT NOT NULL REFERENCES accounts(id),
  fallback_accounts JSONB NOT NULL DEFAULT '[]'::jsonb,
  strategy TEXT NOT NULL DEFAULT 'primary_then_weighted_fallback',
  mode TEXT NOT NULL DEFAULT 'native',
  adapter TEXT,
  allow_lossy_conversion BOOLEAN NOT NULL DEFAULT FALSE,
  enabled BOOLEAN NOT NULL DEFAULT TRUE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
