-- Issue #96: three of the four Sources in this deployment were created
-- with `provider_preset_id='custom'`, which is an empty preset.  The
-- runtime still worked because each Source carried its own `endpoints`
-- JSON, but every recorded `usage_events.provider_id` was rewritten to
-- `'custom'` (migration 0010 copies the Source's `provider_preset_id`
-- onto the event), losing the ability to break usage down by vendor.
--
-- Reassign the built-in v3 presets to the matching Sources and rebuild
-- the snapshot from the preset table.  The per-Source `endpoints` JSON
-- is preserved by the UPDATE.  The `bai` Source has no built-in preset
-- and intentionally stays on `custom|1`.
UPDATE sources AS s
SET provider_preset_id = pp.id,
    provider_preset_version = pp.version,
    provider_preset_snapshot = pp.definition,
    updated_at = NOW()
FROM provider_presets AS pp
WHERE s.provider_preset_id = 'custom'
  AND s.id = pp.id
  AND pp.id IN ('minimax', 'deepseek', 'kimi_code')
  AND pp.version = (
    SELECT MAX(version)
    FROM provider_presets AS latest
    WHERE latest.id = pp.id
  );

-- Re-run the migration-0010 backfill using the now-correct Source mapping.
-- The original 0010 only rewrote rows where provider_id equalled the Source
-- id; this update additionally rewrites the `custom` placeholder written
-- by the original migration, so the column converges on the real vendor id.
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