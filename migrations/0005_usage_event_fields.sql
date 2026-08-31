-- #26: Add route_id, streamed, and error_summary to usage_events.
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS route_id TEXT;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS streamed BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS error_summary TEXT;

CREATE INDEX IF NOT EXISTS idx_usage_events_route_id ON usage_events (route_id);
