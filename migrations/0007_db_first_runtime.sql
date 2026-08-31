-- Complete the Source/Binding-backed control plane used by the runtime.
-- Provider/account/route compatibility columns remain for historical rows,
-- but new control-plane writes use sources and logical model bindings.

ALTER TABLE accounts ALTER COLUMN provider_id DROP NOT NULL;

ALTER TABLE logical_models
  ADD COLUMN IF NOT EXISTS enabled BOOLEAN NOT NULL DEFAULT TRUE;

ALTER TABLE model_bindings
  ADD COLUMN IF NOT EXISTS enabled BOOLEAN NOT NULL DEFAULT TRUE;

ALTER TABLE routes
  ADD COLUMN IF NOT EXISTS logical_model_id TEXT REFERENCES logical_models(id) ON DELETE CASCADE;

ALTER TABLE routes ALTER COLUMN provider_id DROP NOT NULL;
ALTER TABLE routes ALTER COLUMN primary_account_id DROP NOT NULL;

CREATE INDEX IF NOT EXISTS idx_logical_models_runtime
  ON logical_models (public_name)
  WHERE enabled AND status = 'confirmed';

CREATE INDEX IF NOT EXISTS idx_model_bindings_runtime
  ON model_bindings (logical_model_id, protocol, priority DESC, id)
  WHERE enabled AND status = 'confirmed';

CREATE INDEX IF NOT EXISTS idx_routes_runtime
  ON routes (logical_model_id, id)
  WHERE enabled AND logical_model_id IS NOT NULL;

-- Serialize control-plane publications and give each committed snapshot a
-- monotonic identity. App processes use this revision to prevent an older
-- completed request from overwriting a newer in-memory snapshot.
CREATE TABLE IF NOT EXISTS runtime_snapshot_state (
  singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
  revision BIGINT NOT NULL DEFAULT 0 CHECK (revision >= 0),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO runtime_snapshot_state (singleton, revision)
VALUES (TRUE, 0)
ON CONFLICT (singleton) DO NOTHING;

CREATE OR REPLACE FUNCTION validate_confirmed_model_binding()
RETURNS TRIGGER AS $$
BEGIN
  IF NEW.status = 'confirmed' AND NEW.enabled AND NOT EXISTS (
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
      AND lm.enabled
      AND s.enabled
      AND a.enabled
      AND sm.confirmation_status = 'confirmed'
      AND sm.availability_status = 'available'
      AND capability.status = 'confirmed'
      AND capability.mode IN ('native', 'adapter')
  ) THEN
    RAISE EXCEPTION 'confirmed enabled model binding is not routable'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;
