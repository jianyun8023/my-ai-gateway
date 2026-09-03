-- Split the opaque cached_tokens column into cache_read_tokens and
-- cache_creation_tokens so consumers can distinguish cache hits from
-- first-time cache writes.  The existing cached_tokens column is kept
-- as the algebraic sum (cache_read + cache_creation) for backward-
-- compatible queries and billing aggregations.
--
-- Background: MiniMax and Anthropic report cache_read_input_tokens and
-- cache_creation_input_tokens separately; the gateway previously merged
-- them into a single cached_tokens value, making it impossible to tell
-- whether a request hit cache or primed it for the first time.

INSERT INTO gateway_schema_migrations (version, name)
VALUES (20, 'split_cache_tokens')
ON CONFLICT (version) DO NOTHING;

UPDATE gateway_schema_metadata
SET schema_version = GREATEST(schema_version, 20),
    migration_version = GREATEST(migration_version, 20),
    updated_at = NOW()
WHERE singleton = TRUE;

ALTER TABLE usage_events
  ADD COLUMN IF NOT EXISTS cache_read_tokens BIGINT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS cache_creation_tokens BIGINT NOT NULL DEFAULT 0;

COMMENT ON COLUMN usage_events.cached_tokens IS
  'Sum of cache_read_tokens + cache_creation_tokens. Kept for backward-compatible queries; prefer the split columns for analytics.';
COMMENT ON COLUMN usage_events.cache_read_tokens IS
  'Tokens served from prompt cache (Anthropic cache_read_input_tokens / OpenAI cached_tokens). Billed at the cache-read rate.';
COMMENT ON COLUMN usage_events.cache_creation_tokens IS
  'Tokens written into prompt cache for the first time (Anthropic cache_creation_input_tokens). Billed at the cache-creation rate.';
