import { isAbortError } from '@/admin-api/errors';
import type { GatewayUsageClient } from '@/gateway-usage/client';
import { resolveFilterWindow } from '@/gateway-usage/filterState';
import { appendStableEventPage } from '@/gateway-usage/pagination';
import type {
  GatewayUsageFilters, UsageBreakdownDimension, UsageBreakdownItem,
  UsageEventPageViewModel, UsageOverviewViewModel, UsageSummaryViewModel,
} from '@/gateway-usage/types';
import type { GatewayUsageTab } from '@/lib/consoleNavigation';
import { downloadBlob } from '@/utils/download';
import { useCallback, useEffect, useRef, useState } from 'react';
import { ANALYSIS_DIMENSIONS } from './model';

interface UsageData {
  overview?: UsageOverviewViewModel;
  analysisSummary?: UsageSummaryViewModel;
  breakdowns?: Partial<Record<UsageBreakdownDimension, UsageBreakdownItem[]>>;
  eventPage?: UsageEventPageViewModel;
}

interface UsageState extends UsageData {
  queryKey: string;
  loading: boolean;
  refreshing: boolean;
  loadingMore: boolean;
  error?: { cause: unknown; messageKey: string };
  loadMoreError?: { cause: unknown; messageKey: string };
  exportError?: { cause: unknown; messageKey: string; format: 'csv' | 'json' };
  exportingFormat?: 'csv' | 'json';
}

interface QuerySession {
  queryKey: string;
  controller: AbortController;
  filters: GatewayUsageFilters;
  eventPage?: UsageEventPageViewModel;
  loadingMore: boolean;
  failedCursor?: string;
  exporting: boolean;
}

interface LoadSession {
  queryKey: string;
  controller: AbortController;
  filters: GatewayUsageFilters;
}

interface UsageDataOptions {
  client: Pick<GatewayUsageClient, 'overview' | 'summary' | 'breakdown' | 'events' | 'exportEvents'>;
  filters: GatewayUsageFilters;
  activeTab: GatewayUsageTab;
  granularity: 'auto' | 'hour' | 'day';
  authGeneration: number;
  refreshRevision: number;
  onLoadingChange?: (loading: boolean) => void;
}

const usageQueryKey = (
  filters: GatewayUsageFilters,
  activeTab: GatewayUsageTab,
  granularity: 'auto' | 'hour' | 'day',
  authGeneration: number,
) => JSON.stringify([
  authGeneration,
  activeTab,
  activeTab === 'overview' ? granularity : '',
  filters.timeMode ?? 'absolute',
  filters.timeMode === 'relative' ? (filters.relativePreset ?? 'today') : filters.from,
  filters.timeMode === 'relative' ? '' : filters.to,
  filters.logicalModel ?? '',
  filters.upstreamModel ?? '',
  filters.provider ?? '',
  filters.sourceId ?? '',
  filters.clientSource ?? '',
  filters.account ?? '',
  filters.protocolIn ?? '',
  filters.protocolUpstream ?? '',
  filters.virtualKey ?? '',
  filters.status ?? '',
  filters.usageSource ?? '',
]);

const hasTabData = (state: UsageData, activeTab: GatewayUsageTab) => (
  activeTab === 'overview'
    ? state.overview !== undefined
    : activeTab === 'analysis'
      ? state.analysisSummary !== undefined
      : state.eventPage !== undefined
);

export function useUsageData({ client, filters, activeTab, granularity, authGeneration, refreshRevision, onLoadingChange }: UsageDataOptions) {
  const queryKey = usageQueryKey(filters, activeTab, granularity, authGeneration);
  const [state, setState] = useState<UsageState>({ queryKey, loading: true, refreshing: false, loadingMore: false });
  const [reloadRevision, setReloadRevision] = useState(0);
  const session = useRef<QuerySession | undefined>(undefined);
  const pendingSession = useRef<LoadSession | undefined>(undefined);

  const execute = useCallback(async (current: LoadSession) => {
    const signal = current.controller.signal;
    setState((previous) => {
      const preserve = previous.queryKey === current.queryKey && hasTabData(previous, activeTab);
      return {
        ...(preserve ? previous : {}),
        queryKey: current.queryKey,
        loading: !preserve,
        refreshing: preserve,
        loadingMore: false,
        error: undefined,
        loadMoreError: undefined,
        exportError: undefined,
        exportingFormat: undefined,
      };
    });
    onLoadingChange?.(true);
    try {
      let data: UsageData;
      if (activeTab === 'overview') {
        data = { overview: await client.overview(current.filters, granularity === 'auto' ? undefined : granularity, signal) };
      } else if (activeTab === 'analysis') {
        const [analysisSummary, breakdowns] = await Promise.all([
          client.summary(current.filters, signal),
          Promise.all(ANALYSIS_DIMENSIONS.map(async ({ dimension }) => [
            dimension, await client.breakdown(current.filters, dimension, signal),
          ] as const)),
        ]);
        data = { analysisSummary, breakdowns: Object.fromEntries(breakdowns) };
      } else {
        data = { eventPage: await client.events({ filters: current.filters, limit: 100 }, signal) };
      }
      // Fetch implementations may resolve even after abort. Never publish an old session.
      if (signal.aborted || pendingSession.current !== current) return;
      const previous = session.current;
      session.current = {
        queryKey: current.queryKey,
        controller: new AbortController(),
        filters: current.filters,
        eventPage: data.eventPage,
        loadingMore: false,
        exporting: false,
      };
      previous?.controller.abort();
      setState({ ...data, queryKey: current.queryKey, loading: false, refreshing: false, loadingMore: false });
    } catch (cause) {
      if (!signal.aborted && pendingSession.current === current && !isAbortError(cause)) {
        setState((previous) => previous.queryKey === current.queryKey ? {
          ...previous,
          loading: false,
          refreshing: false,
          loadingMore: false,
          error: { cause, messageKey: 'usage.error.load_failed' },
        } : previous);
      }
    } finally {
      if (pendingSession.current === current) pendingSession.current = undefined;
      if (!signal.aborted) onLoadingChange?.(false);
    }
  }, [activeTab, client, granularity, onLoadingChange]);

  useEffect(() => {
    const published = session.current;
    if (published) {
      published.controller.abort();
      session.current = published.queryKey === queryKey ? {
        ...published,
        controller: new AbortController(),
        loadingMore: false,
        failedCursor: undefined,
        exporting: false,
      } : undefined;
    }
    const current: LoadSession = {
      queryKey,
      controller: new AbortController(),
      filters: resolveFilterWindow(filters),
    };
    pendingSession.current = current;
    void execute(current);
    return () => {
      current.controller.abort();
      if (pendingSession.current === current) pendingSession.current = undefined;
      onLoadingChange?.(false);
    };
  }, [execute, filters, onLoadingChange, queryKey, refreshRevision, reloadRevision]);

  useEffect(() => () => {
    session.current?.controller.abort();
    pendingSession.current?.controller.abort();
  }, []);

  const requestMore = useCallback(async (retryCursor?: string) => {
    const current = session.current;
    const previous = current?.eventPage;
    if (!current || current.queryKey !== queryKey || pendingSession.current?.queryKey === queryKey
      || current.controller.signal.aborted || current.loadingMore || !previous?.hasMore || !previous.nextCursor) return;
    const signal = current.controller.signal;
    const cursor = previous.nextCursor;
    if (retryCursor !== undefined && retryCursor !== cursor) return;
    if (retryCursor === undefined && current.failedCursor === cursor) return;
    // Lock synchronously: scroll observers may request the same cursor before React rerenders.
    current.loadingMore = true;
    current.failedCursor = undefined;
    setState((value) => session.current === current && value.queryKey === current.queryKey
      ? ({ ...value, loadingMore: true, loadMoreError: undefined }) : value);
    try {
      const page = await client.events({ filters: current.filters, cursor, limit: 100 }, signal);
      if (signal.aborted || session.current !== current) return;
      const eventPage = { ...page, events: appendStableEventPage(previous.events, page.events) };
      current.eventPage = eventPage;
      setState((value) => session.current === current && value.queryKey === current.queryKey
        ? ({ ...value, eventPage, loadMoreError: undefined }) : value);
    } catch (cause) {
      if (!signal.aborted && session.current === current && !isAbortError(cause)) {
        current.failedCursor = cursor;
        setState((value) => session.current === current && value.queryKey === current.queryKey ? ({
          ...value,
          loadMoreError: { cause, messageKey: 'usage.error.load_more_failed' },
        }) : value);
      }
    } finally {
      current.loadingMore = false;
      if (!signal.aborted && session.current === current) {
        setState((value) => value.queryKey === current.queryKey ? ({ ...value, loadingMore: false }) : value);
      }
    }
  }, [client, queryKey]);

  const loadMore = useCallback(() => requestMore(), [requestMore]);
  const retryLoadMore = useCallback(() => {
    const failedCursor = session.current?.failedCursor;
    if (failedCursor !== undefined) void requestMore(failedCursor);
  }, [requestMore]);

  const performExport = useCallback(async (format: 'csv' | 'json') => {
    const current = session.current;
    if (!current || current.queryKey !== queryKey || current.controller.signal.aborted || current.exporting) return;
    const signal = current.controller.signal;
    current.exporting = true;
    setState((value) => session.current === current && value.queryKey === current.queryKey ? ({
      ...value,
      exportError: undefined,
      exportingFormat: format,
    }) : value);
    try {
      const blob = await client.exportEvents(current.filters, format, signal);
      if (!signal.aborted && session.current === current) downloadBlob(blob, `gateway-usage-events.${format}`);
    } catch (cause) {
      if (!signal.aborted && session.current === current && !isAbortError(cause)) {
        setState((value) => value.queryKey === current.queryKey ? ({
          ...value,
          exportError: { cause, messageKey: 'errors.export_failed', format },
        }) : value);
      }
    } finally {
      current.exporting = false;
      if (!signal.aborted && session.current === current) {
        setState((value) => value.queryKey === current.queryKey ? ({ ...value, exportingFormat: undefined }) : value);
      }
    }
  }, [client, queryKey]);

  const retryExport = useCallback(() => {
    if (state.queryKey === queryKey && state.exportError) void performExport(state.exportError.format);
  }, [performExport, queryKey, state.exportError, state.queryKey]);

  const visibleState: UsageState = state.queryKey === queryKey
    ? state
    : { queryKey, loading: true, refreshing: false, loadingMore: false };

  return {
    ...visibleState,
    loadMore,
    retryLoadMore,
    exportEvents: performExport,
    retryExport,
    reload: () => setReloadRevision((value) => value + 1),
  };
}
