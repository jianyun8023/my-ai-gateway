export type UsageSource = 'upstream' | 'parsed' | 'estimated' | 'missing' | string;

export type UsageStatus = 'success' | 'failure';

export interface GatewayUsageFilters {
  from: string;
  to: string;
  logicalModel?: string;
  upstreamModel?: string;
  provider?: string;
  source?: string;
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
  total: number;
}

export interface UsageSummaryViewModel {
  logicalRequests: number;
  successfulRequests: number;
  failedRequests: number;
  successRate: number;
  upstreamAttempts: number;
  retries: number;
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
  | 'source'
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
}

export interface UsageAttemptViewModel {
  attemptIndex: number;
  provider: string;
  source: string;
  account: string;
  upstreamModel: string;
  protocolUpstream: string;
  statusCode: number;
  success: boolean;
  latencyMs: number;
}

export interface UsageEventViewModel {
  id: string;
  requestId: string;
  createdAt: string;
  logicalModel: string;
  upstreamModel: string;
  provider: string;
  source: string;
  account: string;
  protocolIn: string;
  protocolUpstream: string;
  virtualKey: string;
  statusCode: number;
  success: boolean;
  retryCount: number;
  fallback: boolean;
  latencyMs: number;
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
}

export type RawGatewayUsagePayload = unknown;
