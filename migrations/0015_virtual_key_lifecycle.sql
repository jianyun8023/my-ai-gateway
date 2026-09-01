-- Issue #51: complete the Virtual Key lifecycle.
--
-- A key row represents one credential generation. Rotation keeps the old
-- generation linked through `replaced_by_id` and accepts it only until the
-- explicit `overlap_until` instant. The raw credential is never persisted as
-- plaintext; migration 0016 adds an optional encrypted recovery envelope.

INSERT INTO gateway_schema_migrations (version, name)
VALUES (15, 'virtual_key_lifecycle')
ON CONFLICT (version) DO NOTHING;

UPDATE gateway_schema_metadata
SET schema_version = GREATEST(schema_version, 15),
    migration_version = GREATEST(migration_version, 15),
    updated_at = NOW()
WHERE singleton = TRUE;

ALTER TABLE virtual_keys
  ADD COLUMN IF NOT EXISTS scopes JSONB NOT NULL DEFAULT '["gateway:invoke"]'::jsonb,
  ADD COLUMN IF NOT EXISTS key_group TEXT,
  ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS replaced_by_id BIGINT,
  ADD COLUMN IF NOT EXISTS overlap_until TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  ADD COLUMN IF NOT EXISTS origin TEXT NOT NULL DEFAULT 'created';

-- Existing deployments may have introduced the columns before this migration
-- was replayed. Normalize malformed/null values before adding constraints.
UPDATE virtual_keys
SET scopes = '["gateway:invoke"]'::jsonb
WHERE scopes IS NULL OR jsonb_typeof(scopes) <> 'array';

UPDATE virtual_keys
SET origin = 'created'
WHERE origin IS NULL OR btrim(origin) = '';

ALTER TABLE virtual_keys
  ALTER COLUMN scopes SET DEFAULT '["gateway:invoke"]'::jsonb,
  ALTER COLUMN scopes SET NOT NULL,
  ALTER COLUMN origin SET DEFAULT 'created',
  ALTER COLUMN origin SET NOT NULL;

DO $$
BEGIN
  ALTER TABLE virtual_keys
    ADD CONSTRAINT virtual_keys_replaced_by_fk
    FOREIGN KEY (replaced_by_id) REFERENCES virtual_keys(id) ON DELETE SET NULL;
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
  ALTER TABLE virtual_keys
    ADD CONSTRAINT virtual_keys_scopes_array_check
    CHECK (jsonb_typeof(scopes) = 'array');
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
  ALTER TABLE virtual_keys
    ADD CONSTRAINT virtual_keys_origin_check
    CHECK (origin IN ('created', 'static_migration', 'rotated'));
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;

CREATE INDEX IF NOT EXISTS idx_virtual_keys_active_expiry
  ON virtual_keys (enabled, expires_at, overlap_until);
CREATE INDEX IF NOT EXISTS idx_virtual_keys_replaced_by
  ON virtual_keys (replaced_by_id);
CREATE INDEX IF NOT EXISTS idx_virtual_keys_group
  ON virtual_keys (key_group) WHERE key_group IS NOT NULL;
