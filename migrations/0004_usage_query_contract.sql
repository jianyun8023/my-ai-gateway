ALTER TABLE usage_events
  ADD COLUMN IF NOT EXISTS virtual_key_id BIGINT REFERENCES virtual_keys(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_usage_events_cursor
  ON usage_events (created_at DESC, request_id DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_logical_model_created_at
  ON usage_events (logical_model, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_upstream_model_created_at
  ON usage_events (upstream_model_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_provider_created_at
  ON usage_events (provider_id, created_at DESC);
-- Source and Client Source indexes are owned by 0009. Keeping them out of this
-- earlier, repeatedly embedded script lets an already-upgraded database run
-- the full migration sequence again after the legacy `source` column is gone.
CREATE INDEX IF NOT EXISTS idx_usage_events_account_created_at
  ON usage_events (account_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_protocol_in_created_at
  ON usage_events (protocol_in, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_protocol_upstream_created_at
  ON usage_events (protocol_upstream, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_virtual_key_created_at
  ON usage_events (virtual_key_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_status_created_at
  ON usage_events (success, status_code, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_usage_source_created_at
  ON usage_events (usage_source, created_at DESC);
