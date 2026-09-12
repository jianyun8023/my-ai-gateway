-- Kimi Code CN supports authenticated GET /coding/v1/models (verified
-- 2026-09-12). Register a new immutable preset and update existing Sources
-- once; startup replays must preserve subsequent user changes.
DO $$
DECLARE
  discovery_definition JSONB := '{
    "support": "supported",
    "method": "get",
    "endpoint": "/v1/models",
    "parser": {
      "list_path": "data",
      "id_path": "id",
      "metadata_paths": {"logical_model_name": "id", "display_name": "id"}
    }
  }'::jsonb;
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM gateway_schema_migrations WHERE version = 26
  ) THEN
    -- Migration 0023 registers v4 before the runtime installs builtins.
    INSERT INTO provider_presets (id, version, display_name, definition)
    SELECT id, 5, 'Kimi Code CN',
           jsonb_set(definition, '{discovery}', discovery_definition)
    FROM provider_presets
    WHERE id = 'kimi_code' AND version = 4
    ON CONFLICT (id, version) DO NOTHING;

    -- Only discovery changes in the Source snapshot. Source URL, auth,
    -- endpoints, protocol capabilities, models and routes retain their data.
    UPDATE sources
    SET provider_preset_version = 5,
        provider_preset_snapshot = jsonb_set(
          provider_preset_snapshot, '{discovery}', discovery_definition
        ),
        display_name = CASE WHEN display_name = 'Kimi Code'
          THEN 'Kimi Code CN' ELSE display_name END,
        updated_at = NOW()
    WHERE provider_preset_id = 'kimi_code'
      AND provider_preset_version < 5;

    -- Rename the old default account label while preserving custom names.
    UPDATE accounts AS account
    SET display_name = 'Kimi Code CN', updated_at = NOW()
    FROM sources AS source
    WHERE account.source_id = source.id
      AND source.provider_preset_id = 'kimi_code'
      AND account.display_name = 'Kimi Code';

    INSERT INTO gateway_schema_migrations (version, name)
    VALUES (26, 'kimi_code_cn_discovery');

    UPDATE gateway_schema_metadata
    SET schema_version = GREATEST(schema_version, 26),
        migration_version = GREATEST(migration_version, 26),
        updated_at = NOW()
    WHERE singleton = TRUE;
  END IF;
END $$;
