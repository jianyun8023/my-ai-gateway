DO $$ BEGIN
  CREATE TYPE catalog_status AS ENUM ('pending', 'confirmed', 'unavailable');
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  CREATE TYPE catalog_availability AS ENUM ('unknown', 'available', 'unavailable');
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  CREATE TYPE gateway_protocol AS ENUM (
    'openai_chat_completions',
    'openai_responses',
    'anthropic_messages'
  );
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  CREATE TYPE source_protocol_mode AS ENUM ('unknown', 'native', 'adapter', 'unsupported');
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS provider_presets (
  id TEXT NOT NULL,
  version INTEGER NOT NULL CHECK (version > 0),
  display_name TEXT NOT NULL,
  definition JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (id, version)
);

INSERT INTO provider_presets (id, version, display_name, definition)
VALUES ('custom', 1, 'Custom', '{}'::jsonb)
ON CONFLICT (id, version) DO NOTHING;

CREATE TABLE IF NOT EXISTS sources (
  id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  provider_preset_id TEXT NOT NULL,
  provider_preset_version INTEGER NOT NULL,
  provider_preset_snapshot JSONB NOT NULL,
  base_url TEXT NOT NULL,
  endpoints JSONB NOT NULL DEFAULT '{}'::jsonb,
  auth_config JSONB NOT NULL DEFAULT '{}'::jsonb,
  protocol_capabilities JSONB NOT NULL DEFAULT '{}'::jsonb,
  enabled BOOLEAN NOT NULL DEFAULT TRUE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  FOREIGN KEY (provider_preset_id, provider_preset_version)
    REFERENCES provider_presets(id, version)
);

-- The current control plane calls each Source a provider. Preserve those rows
-- as independent custom-preset snapshots so this migration is forward-only and
-- does not change the running configuration path before Issue #14.
INSERT INTO sources (
  id,
  display_name,
  provider_preset_id,
  provider_preset_version,
  provider_preset_snapshot,
  base_url,
  endpoints,
  protocol_capabilities,
  enabled,
  created_at,
  updated_at
)
SELECT
  id,
  name,
  'custom',
  1,
  jsonb_build_object(
    'base_url', base_url,
    'endpoints', endpoints,
    'capabilities', capabilities
  ),
  base_url,
  endpoints,
  '{}'::jsonb,
  enabled,
  created_at,
  updated_at
FROM providers
ON CONFLICT (id) DO NOTHING;

ALTER TABLE accounts ADD COLUMN IF NOT EXISTS source_id TEXT;
UPDATE accounts SET source_id = provider_id WHERE source_id IS NULL;
ALTER TABLE accounts ALTER COLUMN source_id SET NOT NULL;

DO $$ BEGIN
  ALTER TABLE accounts
    ADD CONSTRAINT accounts_source_id_fkey
    FOREIGN KEY (source_id) REFERENCES sources(id) ON DELETE CASCADE;
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;

CREATE UNIQUE INDEX IF NOT EXISTS uq_accounts_id_source_id ON accounts (id, source_id);

CREATE TABLE IF NOT EXISTS model_presets (
  id TEXT NOT NULL,
  version INTEGER NOT NULL CHECK (version > 0),
  canonical_model_id TEXT NOT NULL,
  aliases JSONB NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(aliases) = 'array'),
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(metadata) = 'object'),
  field_sources JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(field_sources) = 'object'),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (id, version),
  UNIQUE (canonical_model_id, version)
);

CREATE TABLE IF NOT EXISTS source_models (
  source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  upstream_model_id TEXT NOT NULL,
  confirmation_status catalog_status NOT NULL DEFAULT 'pending'
    CHECK (confirmation_status IN ('pending', 'confirmed')),
  availability_status catalog_availability NOT NULL DEFAULT 'unknown',
  raw_snapshot JSONB NOT NULL DEFAULT '{}'::jsonb,
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(metadata) = 'object'),
  field_sources JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(field_sources) = 'object'),
  matched_model_preset_id TEXT,
  matched_model_preset_version INTEGER,
  first_discovered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  last_discovered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  confirmed_at TIMESTAMPTZ,
  unavailable_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (source_id, upstream_model_id),
  FOREIGN KEY (matched_model_preset_id, matched_model_preset_version)
    REFERENCES model_presets(id, version),
  CHECK (
    (matched_model_preset_id IS NULL AND matched_model_preset_version IS NULL)
    OR
    (matched_model_preset_id IS NOT NULL AND matched_model_preset_version IS NOT NULL)
  ),
  CHECK (
    (confirmation_status = 'confirmed' AND confirmed_at IS NOT NULL)
    OR confirmation_status <> 'confirmed'
  ),
  CHECK (
    (availability_status = 'unavailable' AND unavailable_at IS NOT NULL)
    OR availability_status <> 'unavailable'
  )
);

CREATE INDEX IF NOT EXISTS idx_source_models_confirmation_availability
  ON source_models (confirmation_status, availability_status);

CREATE TABLE IF NOT EXISTS logical_models (
  id TEXT PRIMARY KEY,
  public_name TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL,
  status catalog_status NOT NULL DEFAULT 'pending',
  model_preset_id TEXT,
  model_preset_version INTEGER,
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(metadata) = 'object'),
  field_sources JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(field_sources) = 'object'),
  confirmed_at TIMESTAMPTZ,
  unavailable_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  FOREIGN KEY (model_preset_id, model_preset_version)
    REFERENCES model_presets(id, version),
  CHECK (
    (model_preset_id IS NULL AND model_preset_version IS NULL)
    OR
    (model_preset_id IS NOT NULL AND model_preset_version IS NOT NULL)
  ),
  CHECK ((status = 'confirmed' AND confirmed_at IS NOT NULL) OR status <> 'confirmed'),
  CHECK ((status = 'unavailable' AND unavailable_at IS NOT NULL) OR status <> 'unavailable')
);

CREATE TABLE IF NOT EXISTS source_model_capabilities (
  source_id TEXT NOT NULL,
  upstream_model_id TEXT NOT NULL,
  protocol gateway_protocol NOT NULL,
  status catalog_status NOT NULL DEFAULT 'pending',
  mode source_protocol_mode NOT NULL DEFAULT 'unknown',
  source_protocol gateway_protocol,
  adapter TEXT,
  feature_capabilities JSONB NOT NULL DEFAULT '{}'::jsonb
    CHECK (jsonb_typeof(feature_capabilities) = 'object'),
  field_source TEXT NOT NULL DEFAULT 'unknown'
    CHECK (field_source IN ('upstream', 'preset', 'user', 'unknown')),
  observed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  confirmed_at TIMESTAMPTZ,
  unavailable_at TIMESTAMPTZ,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (source_id, upstream_model_id, protocol),
  FOREIGN KEY (source_id, upstream_model_id)
    REFERENCES source_models(source_id, upstream_model_id) ON DELETE CASCADE,
  CHECK (
    (mode = 'adapter' AND source_protocol IS NOT NULL AND adapter IS NOT NULL AND length(trim(adapter)) > 0)
    OR
    (mode <> 'adapter' AND source_protocol IS NULL AND adapter IS NULL)
  ),
  CHECK (mode <> 'adapter' OR source_protocol <> protocol),
  CHECK (status <> 'confirmed' OR mode <> 'unknown'),
  CHECK ((status = 'confirmed' AND confirmed_at IS NOT NULL) OR status <> 'confirmed'),
  CHECK ((status = 'unavailable' AND unavailable_at IS NOT NULL) OR status <> 'unavailable')
);

CREATE TABLE IF NOT EXISTS model_bindings (
  id BIGSERIAL PRIMARY KEY,
  logical_model_id TEXT NOT NULL REFERENCES logical_models(id) ON DELETE CASCADE,
  source_id TEXT NOT NULL,
  account_id TEXT NOT NULL,
  upstream_model_id TEXT NOT NULL,
  protocol gateway_protocol NOT NULL,
  status catalog_status NOT NULL DEFAULT 'pending',
  priority INTEGER NOT NULL DEFAULT 0,
  confirmed_at TIMESTAMPTZ,
  unavailable_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  FOREIGN KEY (source_id, upstream_model_id)
    REFERENCES source_models(source_id, upstream_model_id) ON DELETE CASCADE,
  FOREIGN KEY (source_id, upstream_model_id, protocol)
    REFERENCES source_model_capabilities(source_id, upstream_model_id, protocol),
  FOREIGN KEY (account_id, source_id)
    REFERENCES accounts(id, source_id) ON DELETE CASCADE,
  UNIQUE (logical_model_id, source_id, account_id, upstream_model_id, protocol),
  CHECK ((status = 'confirmed' AND confirmed_at IS NOT NULL) OR status <> 'confirmed'),
  CHECK ((status = 'unavailable' AND unavailable_at IS NOT NULL) OR status <> 'unavailable')
);

CREATE INDEX IF NOT EXISTS idx_model_bindings_routable
  ON model_bindings (logical_model_id, protocol, priority DESC)
  WHERE status = 'confirmed';

CREATE OR REPLACE FUNCTION validate_confirmed_model_binding()
RETURNS TRIGGER AS $$
BEGIN
  IF NEW.status = 'confirmed' AND NOT EXISTS (
    SELECT 1
    FROM logical_models lm
    JOIN sources s ON s.id = NEW.source_id
    JOIN accounts a ON a.id = NEW.account_id AND a.source_id = NEW.source_id
    JOIN source_models sm
      ON sm.source_id = NEW.source_id
     AND sm.upstream_model_id = NEW.upstream_model_id
    JOIN source_model_capabilities capability
      ON capability.source_id = NEW.source_id
     AND capability.upstream_model_id = NEW.upstream_model_id
     AND capability.protocol = NEW.protocol
    WHERE lm.id = NEW.logical_model_id
      AND lm.status = 'confirmed'
      AND s.enabled
      AND a.enabled
      AND sm.confirmation_status = 'confirmed'
      AND sm.availability_status = 'available'
      AND capability.status = 'confirmed'
      AND capability.mode IN ('native', 'adapter')
  ) THEN
    RAISE EXCEPTION 'confirmed model binding is not routable'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_validate_confirmed_model_binding ON model_bindings;
CREATE TRIGGER trg_validate_confirmed_model_binding
BEFORE INSERT OR UPDATE ON model_bindings
FOR EACH ROW EXECUTE FUNCTION validate_confirmed_model_binding();
