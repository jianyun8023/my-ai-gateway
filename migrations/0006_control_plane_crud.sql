-- Expand providers/accounts/routes with fields required for full CRUD
-- so configurations stored in PostgreSQL can fully reconstruct GatewayConfig.

-- providers: store models, native_protocols, protocol_capabilities, model_overrides
ALTER TABLE providers ADD COLUMN IF NOT EXISTS models JSONB NOT NULL DEFAULT '[]'::jsonb;
ALTER TABLE providers ADD COLUMN IF NOT EXISTS native_protocols JSONB NOT NULL DEFAULT '[]'::jsonb;
ALTER TABLE providers ADD COLUMN IF NOT EXISTS protocol_capabilities JSONB NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE providers ADD COLUMN IF NOT EXISTS model_overrides JSONB NOT NULL DEFAULT '{}'::jsonb;

-- accounts: store protocol_capabilities, capabilities, model_overrides, model_map, credential_env
ALTER TABLE accounts ADD COLUMN IF NOT EXISTS protocol_capabilities JSONB NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE accounts ADD COLUMN IF NOT EXISTS capabilities JSONB;
ALTER TABLE accounts ADD COLUMN IF NOT EXISTS model_overrides JSONB NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE accounts ADD COLUMN IF NOT EXISTS model_map JSONB NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE accounts ADD COLUMN IF NOT EXISTS credential_env TEXT;
