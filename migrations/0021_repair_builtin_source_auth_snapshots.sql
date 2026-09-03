-- Issue #127: Sources imported from the legacy control-plane tables were
-- initially stored as custom presets with an empty auth_config. Migration 0017
-- later associated the built-in Sources with their immutable ProviderPreset,
-- but intentionally preserved the per-Source runtime fields. An empty
-- auth_config cannot deserialize into SourceAuthConfig, so connection tests and
-- periodic health probes fail before sending an upstream request.

INSERT INTO gateway_schema_migrations (version, name)
VALUES (21, 'repair_builtin_source_auth_snapshots')
ON CONFLICT (version) DO NOTHING;

UPDATE gateway_schema_metadata
SET schema_version = GREATEST(schema_version, 21),
    migration_version = GREATEST(migration_version, 21),
    updated_at = NOW()
WHERE singleton = TRUE;

-- Only repair the exact empty-object shape produced by the legacy import.
-- Explicit Source-level authentication overrides remain untouched, as do the
-- Source base URL and endpoint overrides.
UPDATE sources AS source
SET auth_config = jsonb_build_object(
      'credential_header', preset.definition -> 'credential_header',
      'default_headers', COALESCE(preset.definition -> 'default_headers', '{}'::jsonb)
    ),
    updated_at = NOW()
FROM provider_presets AS preset
WHERE source.provider_preset_id = preset.id
  AND source.provider_preset_version = preset.version
  AND source.provider_preset_id IN ('deepseek', 'minimax', 'kimi_code')
  AND source.auth_config = '{}'::jsonb
  AND jsonb_typeof(preset.definition -> 'credential_header') = 'object';
