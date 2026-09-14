type UsageSource = 'upstream' | 'parsed' | 'estimated' | 'missing' | string;

type UsageStatus = 'success' | 'failure';

export type UsageRelativePreset = 'today' | 'yesterday' | '24h' | '7d' | '30d';
type UsageTimeMode = 'relative' | 'absolute';

export interface GatewayUsageFilters {
  from: string;
  to: string;
  timeMode?: UsageTimeMode;
  relativePreset?: UsageRelativePreset;
  logicalModel?: string;
  upstreamModel?: string;
  provider?: string;
  sourceId?: string;
  clientSource?: string;
  account?: string;
  protocolIn?: string;
  protocolUpstream?: string;
  virtualKey?: string;
  status?: UsageStatus;
  usageSource?: string;
}

export interface TokenTotals {
  input: number;
  output: number;
  reasoning: number;
  cached: number;
  cacheRead: number;
  cacheCreation: number;
  total: number;
}

export interface UsageSummaryViewModel {
  logicalRequests: number;
  successfulRequests: number;
  failedRequests: number;
  successRate: number;
  upstreamAttempts: number;
  retries: number;
  averageLatencyMs?: number;
  p95LatencyMs?: number;
  tokens: TokenTotals;
  usageSources: Record<string, number>;
}

export interface UsageTimeseriesPoint {
  bucket: string;
  logicalRequests: number;
  upstreamAttempts: number;
  successfulRequests: number;
  tokens: TokenTotals;
}

export type UsageBreakdownDimension =
  | 'logical_model'
  | 'upstream_model'
  | 'provider'
  | 'source_id'
  | 'client_source'
  | 'account'
  | 'protocol_in'
  | 'protocol_upstream'
  | 'virtual_key'
  | 'usage_source';

export interface UsageBreakdownItem {
  key: string;
  label: string;
  logicalRequests: number;
  upstreamAttempts: number;
  successfulRequests: number;
  tokens: TokenTotals;
  averageLatencyMs?: number;
  p95LatencyMs?: number;
}

export interface UsageAttemptViewModel {
  attemptIndex: number;
  provider: string;
  sourceId: string;
  account: string;
  upstreamModel: string;
  protocolUpstream: string;
  statusCode: number;
  success: boolean;
  latencyMs?: number;
}

export interface UsageEventViewModel {
  id: string;
  requestId: string;
  createdAt: string;
  logicalModel: string;
  upstreamModel: string;
  provider: string;
  sourceId: string;
  clientSource: string;
  account: string;
  protocolIn: string;
  protocolUpstream: string;
  virtualKey: string;
  statusCode: number;
  success: boolean;
  retryCount: number;
  fallback: boolean;
  fallbackReason?: string;
  latencyMs?: number;
  // TTFT is recorded for streaming requests only (absent on non-streaming or
  // failed empty streams). Together with latencyMs it derives output TPS.
  ttftMs?: number;
  streamed: boolean;
  tokens: TokenTotals;
  usageSource: UsageSource;
  degraded: boolean;
  errorSummary?: string;
  attempts: UsageAttemptViewModel[];
}

export interface UsageEventPageViewModel {
  events: UsageEventViewModel[];
  nextCursor?: string;
  hasMore: boolean;
}

export interface UsageOverviewViewModel {
  summary: UsageSummaryViewModel;
  timeseries: UsageTimeseriesPoint[];
  recentEvents: UsageEventViewModel[];
  logicalModels: UsageBreakdownItem[];
}

export type RawGatewayUsagePayload = unknown;
