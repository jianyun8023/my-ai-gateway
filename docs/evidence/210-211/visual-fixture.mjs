// Synthetic usage data for the #210/#211 visual review; not real Provider claims.
// Read-only: rejects non-GET requests. Usage shapes mirror web/src/test/fixtures/usage.ts.
import { createServer } from 'node:http';

const startedAt = Date.now();
const iso = (offsetSeconds) => new Date(startedAt - offsetSeconds * 1000).toISOString();

const event = (index, overrides) => ({
  id: `evt-${index}`,
  request_id: `req-${String(index).padStart(3, '0')}`,
  created_at: iso(60 + index * 83),
  logical_model: 'k3-256k',
  upstream_model_id: 'k3-256k',
  provider_id: 'kimi_code',
  source_id: 'kimi-code',
  client_source: 'codex-kimi3',
  account_id: 'kimi-main',
  protocol_in: 'openai_responses',
  protocol_upstream: 'openai_responses',
  status_code: 200,
  success: true,
  retry_count: 0,
  latency_ms: 25000,
  usage_source: 'parsed',
  tokens: { input: 10000, output: 300, reasoning: 120, cached: 9000, cache_read_tokens: 9000, cache_creation_tokens: 0, total: 10300 },
  ...overrides,
});

const events = [
  event(1, {
    latency_ms: 31733,
    tokens: { input: 23373, output: 299, reasoning: 143, cached: 22528, cache_read_tokens: 22528, cache_creation_tokens: 0, total: 23672 },
  }),
  event(2, {
    latency_ms: 26000,
    tokens: { input: 12400, output: 380, reasoning: 0, cached: 10920, cache_read_tokens: 10920, cache_creation_tokens: 0, total: 12780 },
  }),
  event(3, {
    logical_model: 'deepseek-chat', upstream_model_id: 'deepseek-chat', provider_id: 'deepseek', source_id: 'deepseek-main',
    client_source: 'chat-ui', account_id: 'ds-main', protocol_in: 'openai_chat_completions', protocol_upstream: 'openai_chat_completions',
    usage_source: 'upstream', latency_ms: 21000,
    tokens: { input: 8100, output: 620, reasoning: 0, cached: 7380, cache_read_tokens: 7380, cache_creation_tokens: 0, total: 8720 },
  }),
  event(4, {
    usage_source: 'upstream', latency_ms: 23000,
    tokens: { input: 6300, output: 210, reasoning: 0, cached: 4637, cache_read_tokens: 4637, cache_creation_tokens: 0, total: 6510 },
  }),
  event(5, {
    latency_ms: 32000,
    tokens: { input: 18900, output: 450, reasoning: 60, cached: 17965, cache_read_tokens: 17965, cache_creation_tokens: 0, total: 19350 },
  }),
  event(6, {
    usage_source: 'upstream', latency_ms: 25000,
    tokens: { input: 11200, output: 390, reasoning: 0, cached: 10002, cache_read_tokens: 10002, cache_creation_tokens: 0, total: 11590 },
  }),
  event(7, {
    latency_ms: 5300,
    tokens: { input: 2100, output: 120, reasoning: 0, cached: 0, cache_read_tokens: 0, cache_creation_tokens: 0, total: 2220 },
  }),
  event(8, {
    usage_source: 'upstream', latency_ms: 28000,
    tokens: { input: 9700, output: 510, reasoning: 0, cached: 9002, cache_read_tokens: 9002, cache_creation_tokens: 0, total: 10210 },
  }),
  event(9, {
    logical_model: 'claude-sonnet', upstream_model_id: 'claude-sonnet-4', provider_id: 'anthropic', source_id: 'anthropic-tokyo',
    client_source: 'codex-desktop', account_id: 'anthropic-fallback', protocol_in: 'openai_responses', protocol_upstream: 'anthropic_messages',
    usage_source: 'estimated', latency_ms: 1280, retry_count: 1, fallback_reason: 'upstream_http_429',
    tokens: { input: 680, output: 220, reasoning: 80, cached: 120, cache_read_tokens: 100, cache_creation_tokens: 20, total: 980 },
  }),
  event(10, {
    logical_model: 'chat-fast', upstream_model_id: 'fast-v1', provider_id: 'provider-second', source_id: 'second-osaka',
    client_source: 'unknown', account_id: 'main', protocol_in: 'openai_chat_completions', protocol_upstream: 'openai_chat_completions',
    usage_source: 'missing', status_code: 502, success: false, latency_ms: 510,
    tokens: { input: 0, output: 0, reasoning: 0, cached: 0, cache_read_tokens: 0, cache_creation_tokens: 0, total: 0 },
    error_summary: 'Upstream returned a gateway error',
  }),
  event(11, {
    latency_ms: 47000,
    tokens: { input: 1250000, output: 35000, reasoning: 12000, cached: 1140000, cache_read_tokens: 1140000, cache_creation_tokens: 0, total: 1285000 },
  }),
  event(12, {
    usage_source: 'upstream', latency_ms: 4200,
    tokens: { input: 850, output: 90, reasoning: 0, cached: 200, cache_read_tokens: 200, cache_creation_tokens: 0, total: 940 },
  }),
];

const attempt = (index, overrides) => ({
  attempt_index: index,
  provider_id: 'kimi_code',
  source_id: 'kimi-code',
  account_id: 'kimi-main',
  upstream_model_id: 'k3-256k',
  protocol_upstream: 'openai_responses',
  status_code: 200,
  success: true,
  latency_ms: 25000,
  ...overrides,
});

const attemptsByRequest = Object.fromEntries(events.map((item) => {
  if (item.request_id === 'req-009') {
    return [item.request_id, [
      attempt(0, { provider_id: 'anthropic', source_id: 'anthropic-singapore', account_id: 'anthropic-primary', upstream_model_id: 'claude-sonnet-4', protocol_upstream: 'anthropic_messages', status_code: 429, success: false, latency_ms: 310 }),
      attempt(1, { provider_id: 'anthropic', source_id: 'anthropic-tokyo', account_id: 'anthropic-fallback', upstream_model_id: 'claude-sonnet-4', protocol_upstream: 'anthropic_messages', status_code: 200, success: true, latency_ms: 970 }),
    ]];
  }
  if (item.request_id === 'req-010') {
    return [item.request_id, [attempt(0, { provider_id: 'provider-second', source_id: 'second-osaka', account_id: 'main', upstream_model_id: 'fast-v1', protocol_upstream: 'openai_chat_completions', status_code: 502, success: false, latency_ms: 510 })]];
  }
  return [item.request_id, [attempt(0, {
    provider_id: item.provider_id, source_id: item.source_id, account_id: item.account_id,
    upstream_model_id: item.upstream_model_id, protocol_upstream: item.protocol_upstream, latency_ms: item.latency_ms,
  })]];
}));

const sum = (key) => events.reduce((total, item) => total + (item.tokens[key] ?? 0), 0);
const summary = {
  version: 'v1', timezone: 'UTC',
  range: { from: iso(24 * 3600), to: iso(0) },
  data: {
    logical_requests: events.length,
    successes: events.filter((item) => item.success).length,
    failures: events.filter((item) => !item.success).length,
    success_rate: events.filter((item) => item.success).length / events.length,
    upstream_attempts: events.length + 1,
    retries: 1,
    average_latency_ms: 20000,
    p95_latency_ms: 47000,
    input_tokens: sum('input'), output_tokens: sum('output'), reasoning_tokens: sum('reasoning'),
    cached_tokens: sum('cached'), cache_read_tokens: sum('cache_read_tokens'),
    cache_creation_tokens: sum('cache_creation_tokens'), total_tokens: sum('total'),
  },
};

const sourcesBreakdown = {
  version: 'v1', timezone: 'UTC', range: summary.range, dimension: 'usage_source',
  data: ['parsed', 'upstream', 'estimated', 'missing'].map((key) => {
    const rows = events.filter((item) => item.usage_source === key);
    return {
      key, logical_requests: rows.length, upstream_attempts: rows.length, retries: 0,
      successes: rows.filter((item) => item.success).length, failures: rows.filter((item) => !item.success).length,
      success_rate: 1, logical_request_share: rows.length / events.length, total_token_share: 0,
      average_latency_ms: 20000, p95_latency_ms: 47000,
      input_tokens: rows.reduce((total, item) => total + item.tokens.input, 0),
      output_tokens: rows.reduce((total, item) => total + item.tokens.output, 0),
      reasoning_tokens: rows.reduce((total, item) => total + item.tokens.reasoning, 0),
      cached_tokens: rows.reduce((total, item) => total + item.tokens.cached, 0),
      cache_read_tokens: rows.reduce((total, item) => total + item.tokens.cache_read_tokens, 0),
      cache_creation_tokens: rows.reduce((total, item) => total + item.tokens.cache_creation_tokens, 0),
      total_tokens: rows.reduce((total, item) => total + item.tokens.total, 0),
    };
  }),
};

createServer((req, res) => {
  res.setHeader('Content-Type', 'application/json');
  if (req.method !== 'GET') {
    res.writeHead(405);
    res.end(JSON.stringify({ error: { code: 'read_only_fixture', message: 'Read-only visual fixture' } }));
    return;
  }
  const { pathname } = new URL(req.url, 'http://localhost');
  let body;
  if (pathname === '/admin/usage/summary') body = summary;
  else if (pathname === '/admin/usage/breakdown') body = sourcesBreakdown;
  else if (pathname === '/admin/usage/timeseries') body = { version: 'v1', items: [] };
  else if (pathname === '/admin/usage/events') body = { version: 'v1', items: events, next_cursor: undefined, has_more: false };
  else if (pathname.startsWith('/admin/usage/events/')) {
    const requestId = decodeURIComponent(pathname.slice('/admin/usage/events/'.length));
    body = { attempts: attemptsByRequest[requestId] ?? [] };
  } else if (pathname === '/admin/usage/filter-options') body = { data: [], has_more: false };
  else body = { data: [] };
  res.end(JSON.stringify(body));
}).listen(8798, '127.0.0.1', () => console.log('Read-only usage fixture: http://127.0.0.1:8798'));
