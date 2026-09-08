import type { AdminTransport } from '@/admin-api/client';
import {
  adaptUsageBreakdown,
  adaptUsageEventAttempts,
  adaptUsageEventPage,
  adaptUsageSummary,
  adaptUsageTimeseries,
} from './adapter';
import type {
  GatewayUsageFilters,
  UsageAttemptViewModel,
  UsageBreakdownDimension,
  UsageBreakdownItem,
  UsageEventPageViewModel,
  UsageOverviewViewModel,
  UsageSummaryViewModel,
  UsageTimeseriesPoint,
} from './types';

const USAGE_API_ROOT = '/admin/usage';


export interface EventPageRequest {
  filters: GatewayUsageFilters;
  cursor?: string;
  limit?: number;
}

const appendFilters = (params: URLSearchParams, filters: GatewayUsageFilters) => {
  const entries: Array<[string, string | undefined]> = [
    ['from', filters.from],
    ['to', filters.to],
    ['logical_model', filters.logicalModel],
    ['upstream_model', filters.upstreamModel],
    ['provider', filters.provider],
    ['source_id', filters.sourceId],
    ['client_source', filters.clientSource],
    ['account', filters.account],
    ['protocol_in', filters.protocolIn],
    ['protocol_upstream', filters.protocolUpstream],
    ['virtual_key', filters.virtualKey],
    ['status', filters.status],
    ['usage_source', filters.usageSource],
  ];
  for (const [key, value] of entries) {
    if (value) params.set(key, value);
  }
};

export const buildGatewayUsageURL = (
  endpoint: 'summary' | 'timeseries' | 'breakdown' | 'events' | 'export',
  filters: GatewayUsageFilters,
  extras: Record<string, string | number | undefined> = {},
): string => {
  const params = new URLSearchParams();
  appendFilters(params, filters);
  for (const [key, value] of Object.entries(extras)) {
    if (value !== undefined && value !== '') params.set(key, String(value));
  }
  const query = params.toString();
  return `${USAGE_API_ROOT}/${endpoint}${query ? `?${query}` : ''}`;
};

export class GatewayUsageClient {
  constructor(private readonly transport: AdminTransport) {}

  private json(url: string, signal?: AbortSignal): Promise<unknown> {
    return this.transport.json(url, { signal });
  }

  async summary(filters: GatewayUsageFilters, signal?: AbortSignal): Promise<UsageSummaryViewModel> {
    return adaptUsageSummary(await this.json(buildGatewayUsageURL('summary', filters), signal));
  }

  async timeseries(
    filters: GatewayUsageFilters,
    granularity: 'hour' | 'day',
    signal?: AbortSignal,
  ): Promise<UsageTimeseriesPoint[]> {
    return adaptUsageTimeseries(await this.json(
      buildGatewayUsageURL('timeseries', filters, { granularity }),
      signal,
    ));
  }

  async breakdown(
    filters: GatewayUsageFilters,
    dimension: UsageBreakdownDimension,
    signal?: AbortSignal,
  ): Promise<UsageBreakdownItem[]> {
    return adaptUsageBreakdown(await this.json(
      buildGatewayUsageURL('breakdown', filters, { breakdown: dimension }),
      signal,
    ));
  }

  async events(request: EventPageRequest, signal?: AbortSignal): Promise<UsageEventPageViewModel> {
    return adaptUsageEventPage(await this.json(buildGatewayUsageURL('events', request.filters, {
      cursor: request.cursor,
      limit: request.limit ?? 100,
    }), signal));
  }

  async overview(
    filters: GatewayUsageFilters,
    granularityOverride?: 'hour' | 'day',
    signal?: AbortSignal,
  ): Promise<UsageOverviewViewModel> {
    const durationMs = new Date(filters.to).getTime() - new Date(filters.from).getTime();
    const granularity = granularityOverride ?? (durationMs > 3 * 24 * 60 * 60 * 1000 ? 'day' : 'hour');
    const [summary, timeseries, recentEvents, logicalModels] = await Promise.all([
      this.summary(filters, signal),
      this.timeseries(filters, granularity, signal),
      this.events({ filters, limit: 8 }, signal),
      this.breakdown(filters, 'logical_model', signal),
    ]);
    return { summary, timeseries, recentEvents: recentEvents.events, logicalModels };
  }

  async eventDetail(requestId: string, signal?: AbortSignal): Promise<UsageAttemptViewModel[]> {
    return adaptUsageEventAttempts(await this.json(`${USAGE_API_ROOT}/events/${encodeURIComponent(requestId)}`, signal));
  }

  async exportEvents(
    filters: GatewayUsageFilters,
    format: 'csv' | 'json',
    signal?: AbortSignal,
  ): Promise<Blob> {
    const url = buildGatewayUsageURL('export', filters, { format });
    return this.transport.blob(url, { signal, headers: { Accept: format === 'csv' ? 'text/csv' : 'application/json' } });
  }
}
