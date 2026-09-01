-- Persist an encrypted, Admin-recoverable copy of newly issued Virtual Keys.
-- Authentication continues to use key_hash; existing hash-only rows remain
-- valid but cannot be revealed until they are rotated.

INSERT INTO gateway_schema_migrations (version, name)
VALUES (16, 'virtual_key_recovery')
ON CONFLICT (version) DO NOTHING;

UPDATE gateway_schema_metadata
SET schema_version = GREATEST(schema_version, 16),
    migration_version = GREATEST(migration_version, 16),
    updated_at = NOW()
WHERE singleton = TRUE;

ALTER TABLE virtual_keys
  ADD COLUMN IF NOT EXISTS key_ciphertext TEXT;

COMMENT ON COLUMN virtual_keys.key_ciphertext IS
  'AES-GCM envelope used only for explicit Admin reveal; data-plane auth uses key_hash';
