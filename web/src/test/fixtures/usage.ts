export const gatewayUsageSummaryFixture = {
  version: 'v1', timezone: 'UTC',
  range: { from: '2026-08-30T00:00:00Z', to: '2026-08-31T00:00:00Z' },
  data: {
    logical_requests: 3, successes: 2, failures: 1, success_rate: 2 / 3, upstream_attempts: 5, retries: 2,
    average_latency_ms: 940, p95_latency_ms: 1280,
    input_tokens: 1200, output_tokens: 420, reasoning_tokens: 160, cached_tokens: 300,
    cache_read_tokens: 250, cache_creation_tokens: 50, total_tokens: 1780,
  },
};
export const gatewayUsageSourcesFixture = {
  version: 'v1', timezone: 'UTC', range: gatewayUsageSummaryFixture.range,
  dimension: 'usage_source',
  data: [
    {
      key: 'upstream', logical_requests: 1, upstream_attempts: 1, retries: 0,
      successes: 1, failures: 0, success_rate: 1, logical_request_share: 1 / 3, total_token_share: 800 / 1780,
      average_latency_ms: 1040, p95_latency_ms: 1040,
      input_tokens: 520, output_tokens: 200, reasoning_tokens: 80, cached_tokens: 180,
      cache_read_tokens: 150, cache_creation_tokens: 30, total_tokens: 800,
    },
    {
      key: 'estimated', logical_requests: 1, upstream_attempts: 2, retries: 1,
      successes: 1, failures: 0, success_rate: 1, logical_request_share: 1 / 3, total_token_share: 980 / 1780,
      average_latency_ms: 1280, p95_latency_ms: 1280,
      input_tokens: 680, output_tokens: 220, reasoning_tokens: 80, cached_tokens: 120,
      cache_read_tokens: 100, cache_creation_tokens: 20, total_tokens: 980,
    },
    {
      key: 'missing', logical_requests: 1, upstream_attempts: 2, retries: 1,
      successes: 0, failures: 1, success_rate: 0, logical_request_share: 1 / 3, total_token_share: 0,
      average_latency_ms: 500, p95_latency_ms: 500,
      input_tokens: 0, output_tokens: 0, reasoning_tokens: 0, cached_tokens: 0,
      cache_read_tokens: 0, cache_creation_tokens: 0, total_tokens: 0,
    },
  ],
};
export const gatewayUsageTimeseriesFixture = {
  version: 'v1',
  items: [
    {
      bucket: '2026-08-30T10:00:00Z',
      logical_requests: 2,
      upstream_attempts: 3,
      successful_requests: 2,
      tokens: { input: 1000, output: 400, reasoning: 160, cached: 300, cache_read_tokens: 200, cache_creation_tokens: 100, total: 1560 },
    },
    {
      bucket: '2026-08-30T11:00:00Z',
      logical_requests: 1,
      upstream_attempts: 2,
      successful_requests: 0,
      tokens: { input: 200, output: 20, reasoning: 0, cached: 0, cache_read_tokens: 0, cache_creation_tokens: 0, total: 220 },
    },
  ],
};

export const gatewayUsageBreakdownFixture = {
  version: 'v1',
  dimension: 'logical_model',
  items: [
    {
      key: 'reasoning-large',
      label: 'reasoning-large',
      logical_requests: 3,
      upstream_attempts: 5,
      successful_requests: 2,
      average_latency_ms: 940,
      tokens: { input: 1200, output: 420, reasoning: 160, cached: 300, cache_read_tokens: 250, cache_creation_tokens: 50, total: 1780 },
    },
  ],
};

export const gatewayUsageEventsFixture = {
  version: 'v1',
  items: [
    {
      id: 'evt-1',
      request_id: 'req-fallback',
      created_at: '2026-08-30T10:03:00Z',
      logical_model: 'reasoning-large',
      upstream_model_id: 'provider-model-v2',
      provider_id: 'provider-fallback',
      source_id: 'source-tokyo',
      client_source: 'codex-desktop',
      account_id: 'fallback-account',
      protocol_in: 'openai_responses',
      protocol_upstream: 'anthropic_messages',
      status_code: 200,
      success: true,
      retry_count: 1,
      fallback_reason: 'upstream_http_429',
      latency_ms: 1280,
      usage_source: 'estimated',
      tokens: { input: 680, output: 220, reasoning: 80, cached: 120, cache_read_tokens: 100, cache_creation_tokens: 20, total: 980 },
      attempts: [
        {
          attempt_index: 0,
          provider_id: 'provider-primary',
          source_id: 'source-singapore',
          account_id: 'primary-account',
          upstream_model_id: 'provider-model-v2',
          protocol_upstream: 'anthropic_messages',
          status_code: 429,
          success: false,
          latency_ms: 310,
        },
        {
          attempt_index: 1,
          provider_id: 'provider-fallback',
          source_id: 'source-tokyo',
          account_id: 'fallback-account',
          upstream_model_id: 'provider-model-v2',
          protocol_upstream: 'anthropic_messages',
          status_code: 200,
          success: true,
          latency_ms: 970,
        },
      ],
    },
    {
      id: 'evt-2',
      request_id: 'req-missing',
      created_at: '2026-08-30T10:02:00Z',
      logical_model: 'chat-fast',
      upstream_model_id: 'fast-v1',
      provider_id: 'provider-second',
      source_id: 'source-osaka',
      client_source: 'unknown',
      account_id: 'main',
      protocol_in: 'openai_chat_completions',
      protocol_upstream: 'openai_chat_completions',
      status_code: 502,
      success: false,
      retry_count: 0,
      latency_ms: 510,
      usage_source: 'missing',
      tokens: { input: 0, output: 0, reasoning: 0, cached: 0, cache_read_tokens: 0, cache_creation_tokens: 0, total: 0 },
      error_summary: 'Upstream returned a gateway error',
    },
  ],
  next_cursor: 'cursor-2',
  has_more: true,
};
