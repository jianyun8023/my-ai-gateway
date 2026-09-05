-- Issue #96: repair Sources created with provider_preset_id='custom' when a
-- matching built-in preset exists, and backfill usage attribution columns.
--
-- Execution (review the script first, then run against production):
--   psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f scripts/fix-96-provider-attribution.sql
--
-- Idempotent: each UPDATE only touches rows still on the stale mapping
-- (sources stuck on custom|1 for built-in ids; usage rows still showing
-- 'custom' or the raw source_id placeholder from migration 0010).
-- The `bai` Source has no built-in preset and intentionally stays custom|1.

BEGIN;

-- Point built-in Sources at the latest immutable preset snapshot (v3).
UPDATE sources AS s
SET provider_preset_id = pp.id,
    provider_preset_version = pp.version,
    provider_preset_snapshot = pp.definition,
    updated_at = NOW()
FROM provider_presets AS pp
WHERE s.provider_preset_id = 'custom'
  AND s.provider_preset_version = 1
  AND s.id = pp.id
  AND pp.id IN ('minimax', 'deepseek', 'kimi_code')
  AND pp.version = (
    SELECT MAX(version)
    FROM provider_presets AS latest
    WHERE latest.id = pp.id
  );

-- Re-attribute usage events from the stale custom/source_id placeholders.
UPDATE usage_events AS event
SET provider_id = COALESCE(
  (SELECT source.provider_preset_id FROM sources AS source WHERE source.id = event.source_id),
  'unknown'
)
WHERE event.source_id IS NOT NULL
  AND event.provider_id IN ('custom', event.source_id);

UPDATE usage_event_attempts AS attempt
SET provider_id = COALESCE(
  (SELECT source.provider_preset_id FROM sources AS source WHERE source.id = attempt.source_id),
  'unknown'
)
WHERE attempt.source_id IS NOT NULL
  AND attempt.provider_id IN ('custom', attempt.source_id);

COMMIT;
