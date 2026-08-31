-- Split the legacy client-reported `source` value from the first-class Source
-- selected by the DB-first runtime. Historical rows retain their original
-- client attribution while their runtime Source remains explicitly unknown.

DO $$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM information_schema.columns
    WHERE table_schema = current_schema()
      AND table_name = 'usage_events'
      AND column_name = 'source'
  ) AND NOT EXISTS (
    SELECT 1
    FROM information_schema.columns
    WHERE table_schema = current_schema()
      AND table_name = 'usage_events'
      AND column_name = 'client_source'
  ) THEN
    ALTER TABLE usage_events RENAME COLUMN source TO client_source;
  END IF;
END $$;

ALTER TABLE usage_events
  ADD COLUMN IF NOT EXISTS client_source TEXT NOT NULL DEFAULT 'unknown';
ALTER TABLE usage_events
  ADD COLUMN IF NOT EXISTS source_id TEXT;
ALTER TABLE usage_event_attempts
  ADD COLUMN IF NOT EXISTS source_id TEXT;

-- Usage history intentionally has no FK to sources. Deleting a control-plane
-- Source must not erase or null out historical attribution.
DROP INDEX IF EXISTS idx_usage_events_source_created_at;
CREATE INDEX IF NOT EXISTS idx_usage_events_source_id_created_at
  ON usage_events (source_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_client_source_created_at
  ON usage_events (client_source, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_event_attempts_source_id_created_at
  ON usage_event_attempts (source_id, created_at DESC);
