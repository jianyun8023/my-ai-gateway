-- Issue #100: `usage_events.total_tokens` is the upstream-reported or
-- estimated sum of `input_tokens + output_tokens`.  Providers that bill
-- prompt-cache hits separately (Anthropic `cache_read_input_tokens`,
-- DeepSeek/MiniMax `cache_creation_input_tokens`) report those tokens
-- under `cached_tokens`, so a session that reads back 295k cached tokens
-- only shows up as `input_tokens=444` / `cached_tokens=295552` /
-- `total_tokens=1037`.  Document the contract so downstream billing
-- dashboards do not silently undercount cached reads.
COMMENT ON COLUMN usage_events.total_tokens IS
  'input_tokens + output_tokens. Excludes cached_tokens (Anthropic cache_read_input_tokens, cache_creation_input_tokens). For billed totals across providers, sum input_tokens + output_tokens + cached_tokens.';
COMMENT ON COLUMN usage_events.cached_tokens IS
  'Anthropic-style cache hits: cache_read_input_tokens + cache_creation_input_tokens. Always excluded from total_tokens; downstream consumers must add it back when computing billed totals.';
COMMENT ON COLUMN usage_events.input_tokens IS
  'New (uncached) prompt tokens billed by the upstream. Excludes Anthropic cache_read_input_tokens and cache_creation_input_tokens, which live in cached_tokens.';
COMMENT ON COLUMN usage_events.output_tokens IS
  'Generated tokens billed by the upstream. Excludes reasoning/thinking tokens on providers that surface them separately; those live in reasoning_tokens.';