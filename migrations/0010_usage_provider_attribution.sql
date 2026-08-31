-- DB-first snapshots previously reused Source IDs as provider_id. Only rewrite
-- rows that carry that exact bad signature; pre-Source history stays intact.
UPDATE usage_events AS event
SET provider_id = COALESCE(
  (SELECT source.provider_preset_id FROM sources AS source WHERE source.id = event.source_id),
  'unknown'
)
WHERE event.source_id IS NOT NULL
  AND event.provider_id = event.source_id;

UPDATE usage_event_attempts AS attempt
SET provider_id = COALESCE(
  (SELECT source.provider_preset_id FROM sources AS source WHERE source.id = attempt.source_id),
  'unknown'
)
WHERE attempt.source_id IS NOT NULL
  AND attempt.provider_id = attempt.source_id;
