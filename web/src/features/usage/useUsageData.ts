import { isAbortError } from '@/admin-api/errors';
import type { GatewayUsageClient } from '@/gateway-usage/client';
import { resolveFilterWindow } from '@/gateway-usage/filterState';
import { appendStableEventPage } from '@/gateway-usage/pagination';
import type {
  GatewayUsageFilters, UsageBreakdownDimension, UsageBreakdownItem,
  UsageEventPageViewModel, UsageOverviewViewModel, UsageSummaryViewModel,
} from '@/gateway-usage/types';
import { useQuerySession, type QuerySession } from '@/hooks/useQuerySession';
import type { GatewayUsageTab } from '@/lib/consoleNavigation';
import { downloadBlob } from '@/utils/download';
import { useCallback, useRef, useState } from 'react';
import { ANALYSIS_DIMENSIONS } from './model';

interface UsageData {
  filters: GatewayUsageFilters;
  overview?: UsageOverviewViewModel;
  analysisSummary?: UsageSummaryViewModel;
  breakdowns?: Partial<Record<UsageBreakdownDimension, UsageBreakdownItem[]>>;
  eventPage?: UsageEventPageViewModel;
}

interface UsageActions {
  session?: QuerySession<UsageData>;
  loadingMore?: boolean;
  loadMoreError?: { cause: unknown; messageKey: string };
  exportError?: { cause: unknown; messageKey: string; format: 'csv' | 'json' };
  exportingFormat?: 'csv' | 'json';
}

interface UsageContinuation {
  scope: QuerySession<UsageData>;
  eventPage?: UsageEventPageViewModel;
  loadingMore: boolean;
  failedCursor?: string;
  exporting: boolean;
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

export function useUsageData({ client, filters, activeTab, granularity, authGeneration, refreshRevision, onLoadingChange }: UsageDataOptions) {
  const queryKey = usageQueryKey(filters, activeTab, granularity, authGeneration);
  const load = useCallback(async (signal: AbortSignal): Promise<UsageData> => {
    const window = resolveFilterWindow(filters);
    if (activeTab === 'overview') {
      return { filters: window, overview: await client.overview(window, granularity === 'auto' ? undefined : granularity, signal) };
    }
    if (activeTab === 'analysis') {
      const [analysisSummary, breakdowns] = await Promise.all([
        client.summary(window, signal),
        Promise.all(ANALYSIS_DIMENSIONS.map(async ({ dimension }) => [
          dimension, await client.breakdown(window, dimension, signal),
        ] as const)),
      ]);
      return { filters: window, analysisSummary, breakdowns: Object.fromEntries(breakdowns) };
    }
    return { filters: window, eventPage: await client.events({ filters: window, limit: 100 }, signal) };
  }, [activeTab, client, filters, granularity]);
  const query = useQuerySession({ load, queryKey, refreshRevision, onBusyChange: onLoadingChange });
  const { getSession } = query;
  const continuation = useRef<UsageContinuation | undefined>(undefined);
  const [pageState, setPageState] = useState<{ base?: UsageData; page?: UsageEventPageViewModel }>({});
  const [actions, setActions] = useState<UsageActions>({});

  const getContinuation = useCallback(() => {
    const scope = getSession();
    if (!scope) return undefined;
    if (continuation.current?.scope !== scope) {
      const previous = continuation.current;
      continuation.current = {
        scope,
        // Refresh failure keeps the published page and its original cursor/window.
        eventPage: previous?.scope.data === scope.data ? previous.eventPage : scope.data.eventPage,
        loadingMore: false,
        exporting: false,
      };
    }
    return continuation.current;
  }, [getSession]);
  const updateActions = useCallback((current: UsageContinuation, patch: Partial<UsageActions>) => {
    setActions((previous) => current.scope.isCurrent()
      ? { ...(previous.session === current.scope ? previous : {}), session: current.scope, ...patch }
      : previous);
  }, []);

  const requestMore = useCallback(async (retryCursor?: string) => {
    const current = getContinuation();
    const previous = current?.eventPage;
    if (!current || query.loading || query.refreshing || current.loadingMore || !previous?.hasMore || !previous.nextCursor) return;
    const cursor = previous.nextCursor;
    if (retryCursor !== undefined && retryCursor !== cursor) return;
    if (retryCursor === undefined && current.failedCursor === cursor) return;
    // Scroll observers may fire again before React renders the busy state.
    current.loadingMore = true;
    current.failedCursor = undefined;
    updateActions(current, { loadingMore: true, loadMoreError: undefined });
    try {
      const page = await client.events({ filters: current.scope.data.filters, cursor, limit: 100 }, current.scope.signal);
      if (!current.scope.isCurrent()) return;
      const eventPage = { ...page, events: appendStableEventPage(previous.events, page.events) };
      current.eventPage = eventPage;
      setPageState((value) => current.scope.isCurrent() ? { base: current.scope.data, page: eventPage } : value);
    } catch (cause) {
      if (current.scope.isCurrent() && !isAbortError(cause)) {
        current.failedCursor = cursor;
        updateActions(current, { loadMoreError: { cause, messageKey: 'usage.error.load_more_failed' } });
      }
    } finally {
      current.loadingMore = false;
      updateActions(current, { loadingMore: false });
    }
  }, [client, getContinuation, query.loading, query.refreshing, updateActions]);
  const loadMore = useCallback(() => requestMore(), [requestMore]);
  const retryLoadMore = useCallback(() => {
    const failedCursor = getContinuation()?.failedCursor;
    if (failedCursor !== undefined) void requestMore(failedCursor);
  }, [getContinuation, requestMore]);

  const performExport = useCallback(async (format: 'csv' | 'json') => {
    const current = getContinuation();
    if (!current || current.exporting) return;
    current.exporting = true;
    updateActions(current, { exportError: undefined, exportingFormat: format });
    try {
      const blob = await client.exportEvents(current.scope.data.filters, format, current.scope.signal);
      if (current.scope.isCurrent()) downloadBlob(blob, `gateway-usage-events.${format}`);
    } catch (cause) {
      if (current.scope.isCurrent() && !isAbortError(cause)) {
        updateActions(current, { exportError: { cause, messageKey: 'errors.export_failed', format } });
      }
    } finally {
      current.exporting = false;
      updateActions(current, { exportingFormat: undefined });
    }
  }, [client, getContinuation, updateActions]);
  const visibleActions = actions.session === query.session ? actions : {};
  const retryExport = useCallback(() => {
    if (visibleActions.exportError) void performExport(visibleActions.exportError.format);
  }, [performExport, visibleActions.exportError]);

  return {
    overview: query.data?.overview,
    analysisSummary: query.data?.analysisSummary,
    breakdowns: query.data?.breakdowns,
    eventPage: pageState.base === query.data ? pageState.page : query.data?.eventPage,
    loading: query.loading,
    refreshing: query.refreshing,
    error: query.error === undefined ? undefined : { cause: query.error, messageKey: 'usage.error.load_failed' },
    loadMoreError: visibleActions.loadMoreError,
    exportError: visibleActions.exportError,
    exportingFormat: visibleActions.exportingFormat,
    loadingMore: visibleActions.loadingMore ?? false,
    loadMore, retryLoadMore, exportEvents: performExport, retryExport, reload: query.reload,
  };
}
