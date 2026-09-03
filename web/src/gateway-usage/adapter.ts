import type {
  RawGatewayUsagePayload,
  TokenTotals,
  UsageAttemptViewModel,
  UsageBreakdownItem,
  UsageEventPageViewModel,
  UsageEventViewModel,
  UsageSummaryViewModel,
  UsageTimeseriesPoint,
} from './types';

type UnknownRecord = Record<string, unknown>;

const EMPTY_TOKENS: TokenTotals = { input: 0, output: 0, reasoning: 0, cached: 0, cacheRead: 0, cacheCreation: 0, total: 0 };

const asRecord = (value: unknown): UnknownRecord => (
  value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as UnknownRecord
    : {}
);

const asArray = (value: unknown): unknown[] => Array.isArray(value) ? value : [];

const asString = (value: unknown, fallback = ''): string => (
  typeof value === 'string' && value.trim() ? value : fallback
);

const asNumber = (value: unknown, fallback = 0): number => {
  const parsed = typeof value === 'number' ? value : Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
};

const asBoolean = (value: unknown, fallback = false): boolean => (
  typeof value === 'boolean' ? value : fallback
);

const firstDefined = (record: UnknownRecord, keys: readonly string[]): unknown => {
  for (const key of keys) {
    if (record[key] !== undefined && record[key] !== null) return record[key];
  }
  return undefined;
};

const readNumber = (record: UnknownRecord, keys: readonly string[], fallback = 0): number => (
  asNumber(firstDefined(record, keys), fallback)
);

const readString = (record: UnknownRecord, keys: readonly string[], fallback = ''): string => (
  asString(firstDefined(record, keys), fallback)
);

export const adaptTokenTotals = (payload: RawGatewayUsagePayload): TokenTotals => {
  const root = asRecord(payload);
  const tokens = Object.keys(asRecord(root.tokens)).length > 0 ? asRecord(root.tokens) : root;
  const input = readNumber(tokens, ['input', 'input_tokens']);
  const output = readNumber(tokens, ['output', 'output_tokens']);
  const reasoning = readNumber(tokens, ['reasoning', 'reasoning_tokens']);
  const cacheRead = readNumber(tokens, ['cacheRead', 'cache_read_tokens', 'cache_read']);
  const cacheCreation = readNumber(tokens, ['cacheCreation', 'cache_creation_tokens', 'cache_creation']);
  const cached = readNumber(tokens, ['cached', 'cached_tokens']) || (cacheRead + cacheCreation);
  const explicitTotal = firstDefined(tokens, ['total', 'total_tokens']);
  return {
    input,
    output,
    reasoning,
    cached,
    cacheRead,
    cacheCreation,
    total: explicitTotal === undefined ? input + output : asNumber(explicitTotal),
  };
};

export const adaptUsageSummary = (payload: RawGatewayUsagePayload): UsageSummaryViewModel => {
  const root = asRecord(payload);
  const summary = Object.keys(asRecord(root.data)).length > 0
    ? asRecord(root.data)
    : Object.keys(asRecord(root.summary)).length > 0
    ? asRecord(root.summary)
    : Object.keys(asRecord(root.aggregate)).length > 0
      ? asRecord(root.aggregate)
      : root;
  const logical = asRecord(summary.logical_requests);
  const attempts = asRecord(summary.upstream_attempts);
  const logicalRequests = Object.keys(logical).length > 0
    ? readNumber(logical, ['total', 'requests'])
    : readNumber(summary, ['logical_requests', 'requests', 'total_requests']);
  const successfulRequests = Object.keys(logical).length > 0
    ? readNumber(logical, ['successes', 'successful', 'success'])
    : readNumber(summary, ['successful_requests', 'successes', 'success_count']);
  const failedRequests = Object.keys(logical).length > 0
    ? readNumber(logical, ['failures', 'failed'])
    : readNumber(summary, ['failed_requests', 'failures'], Math.max(0, logicalRequests - successfulRequests));
  const upstreamAttempts = Object.keys(attempts).length > 0
    ? readNumber(attempts, ['total', 'attempts'])
    : readNumber(summary, ['upstream_attempts', 'attempts'], logicalRequests);
  const retries = Object.keys(attempts).length > 0
    ? readNumber(attempts, ['retries'])
    : readNumber(summary, ['retries', 'retry_count'], Math.max(0, upstreamAttempts - logicalRequests));
  const usageSourceRows = asArray(firstDefined(summary, ['usage_sources', 'usage_source_breakdown']));
  const usageSources = Object.fromEntries(usageSourceRows.map((row) => {
    const item = asRecord(row);
    return [readString(item, ['key', 'usage_source', 'dimension'], 'unknown'), readNumber(item, ['requests', 'count'])];
  }));

  return {
    logicalRequests,
    successfulRequests,
    failedRequests,
    successRate: logicalRequests > 0 ? successfulRequests / logicalRequests : 0,
    upstreamAttempts,
    retries,
    averageLatencyMs: readNumber(summary, ['average_latency_ms', 'avg_latency_ms']),
    p95LatencyMs: readNumber(summary, ['p95_latency_ms']),
    tokens: adaptTokenTotals(summary),
    usageSources,
  };
};

export const adaptUsageTimeseries = (payload: RawGatewayUsagePayload): UsageTimeseriesPoint[] => {
  const root = asRecord(payload);
  const rows = asArray(firstDefined(root, ['items', 'timeseries', 'buckets', 'data']));
  return rows.map((row) => {
    const item = asRecord(row);
    const logicalRequests = readNumber(item, ['logical_requests', 'requests']);
    return {
      bucket: readString(item, ['bucket', 'timestamp', 'created_at']),
      logicalRequests,
      upstreamAttempts: readNumber(item, ['upstream_attempts', 'attempts'], logicalRequests),
      successfulRequests: readNumber(item, ['successful_requests', 'successes']),
      tokens: adaptTokenTotals(item),
    };
  }).filter((item) => item.bucket !== '');
};

export const adaptUsageBreakdown = (payload: RawGatewayUsagePayload): UsageBreakdownItem[] => {
  const root = asRecord(payload);
  const rows = asArray(firstDefined(root, ['items', 'breakdown', 'data']));
  return rows.map((row) => {
    const item = asRecord(row);
    const logicalRequests = readNumber(item, ['logical_requests', 'requests']);
    const key = readString(item, ['key', 'dimension', 'id', 'value'], 'unknown');
    return {
      key,
      label: readString(item, ['label', 'name'], key),
      logicalRequests,
      upstreamAttempts: readNumber(item, ['upstream_attempts', 'attempts'], logicalRequests),
      successfulRequests: readNumber(item, ['successful_requests', 'successes']),
      tokens: adaptTokenTotals(item),
      averageLatencyMs: firstDefined(item, ['average_latency_ms', 'avg_latency_ms']) === undefined
        ? undefined
        : readNumber(item, ['average_latency_ms', 'avg_latency_ms']),
    };
  });
};

const adaptAttempt = (payload: unknown, fallbackIndex: number): UsageAttemptViewModel => {
  const item = asRecord(payload);
  return {
    attemptIndex: readNumber(item, ['attempt_index', 'attempt_no'], fallbackIndex),
    provider: readString(item, ['provider', 'provider_name', 'provider_id'], '—'),
    sourceId: readString(item, ['source_id'], 'unknown'),
    account: readString(item, ['account_name', 'account_id'], '—'),
    upstreamModel: readString(item, ['upstream_model', 'upstream_model_id'], '—'),
    protocolUpstream: readString(item, ['protocol_upstream', 'protocol_out'], '—'),
    statusCode: readNumber(item, ['status_code']),
    success: asBoolean(item.success, readNumber(item, ['status_code']) < 400),
    latencyMs: readNumber(item, ['latency_ms']),
  };
};

export const adaptUsageEvent = (payload: RawGatewayUsagePayload): UsageEventViewModel => {
  const item = asRecord(payload);
  const requestId = readString(item, ['request_id', 'id'], 'unknown');
  const attempts = asArray(item.attempts).map(adaptAttempt);
  const retryCount = readNumber(item, ['retry_count', 'retries'], Math.max(0, attempts.length - 1));
  const statusCode = readNumber(item, ['status_code']);
  const success = asBoolean(item.success, statusCode > 0 && statusCode < 400);
  const fallbackReason = readString(item, ['fallback_reason']) || undefined;
  return {
    id: readString(item, ['id'], requestId),
    requestId,
    createdAt: readString(item, ['created_at', 'timestamp']),
    logicalModel: readString(item, ['logical_model', 'model'], '—'),
    upstreamModel: readString(item, ['upstream_model_id', 'upstream_model'], '—'),
    provider: readString(item, ['provider_name', 'provider', 'provider_id'], '—'),
    sourceId: readString(item, ['source_id'], 'unknown'),
    clientSource: readString(item, ['client_source'], 'unknown'),
    account: readString(item, ['account_name', 'account_id'], '—'),
    protocolIn: readString(item, ['protocol_in'], '—'),
    protocolUpstream: readString(item, ['protocol_upstream', 'protocol_out'], '—'),
    virtualKey: readString(item, ['virtual_key_name', 'virtual_key_id'], '—'),
    statusCode,
    success,
    retryCount,
    fallback: asBoolean(item.fallback, retryCount > 0 || attempts.length > 1 || Boolean(fallbackReason)),
    fallbackReason,
    latencyMs: readNumber(item, ['latency_ms']),
    tokens: adaptTokenTotals(item),
    usageSource: readString(item, ['usage_source'], 'missing'),
    degraded: asBoolean(item.degraded),
    errorSummary: readString(item, ['error_summary', 'error'], '') || undefined,
    attempts,
  };
};

export const adaptUsageEventPage = (payload: RawGatewayUsagePayload): UsageEventPageViewModel => {
  const root = asRecord(payload);
  const rows = asArray(firstDefined(root, ['items', 'events', 'data']));
  const page = asRecord(root.page);
  const nextCursor = readString(Object.keys(page).length > 0 ? page : root, ['next_cursor'], '') || undefined;
  return {
    events: rows.map(adaptUsageEvent),
    nextCursor,
    hasMore: asBoolean(Object.keys(page).length > 0 ? page.has_more : root.has_more, Boolean(nextCursor)),
  };
};

export const emptyUsageSummary = (): UsageSummaryViewModel => ({
  logicalRequests: 0,
  successfulRequests: 0,
  failedRequests: 0,
  successRate: 0,
  upstreamAttempts: 0,
  retries: 0,
  averageLatencyMs: 0,
  p95LatencyMs: 0,
  tokens: { ...EMPTY_TOKENS },
  usageSources: {},
});
