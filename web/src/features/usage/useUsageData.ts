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
  loading: boolean;
  loadingMore: boolean;
  error?: { cause: unknown; messageKey: string };
}

interface QuerySession {
  controller: AbortController;
  filters: GatewayUsageFilters;
  eventPage?: UsageEventPageViewModel;
  loadingMore: boolean;
}

interface UsageDataOptions {
  client: Pick<GatewayUsageClient, 'overview' | 'summary' | 'breakdown' | 'events' | 'exportEvents'>;
  filters: GatewayUsageFilters;
  activeTab: GatewayUsageTab;
  granularity: 'auto' | 'hour' | 'day';
  refreshRevision: number;
  onLoadingChange?: (loading: boolean) => void;
}

export function useUsageData({ client, filters, activeTab, granularity, refreshRevision, onLoadingChange }: UsageDataOptions) {
  const [state, setState] = useState<UsageState>({ loading: true, loadingMore: false });
  const [reloadRevision, setReloadRevision] = useState(0);
  const session = useRef<QuerySession | undefined>(undefined);

  const execute = useCallback(async (current: QuerySession) => {
    const signal = current.controller.signal;
    setState({ loading: true, loadingMore: false });
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
      if (signal.aborted) return;
      current.eventPage = data.eventPage;
      setState({ ...data, loading: false, loadingMore: false });
    } catch (cause) {
      if (!signal.aborted && !isAbortError(cause)) {
        setState({ loading: false, loadingMore: false, error: { cause, messageKey: 'usage.error.load_failed' } });
      }
    } finally {
      if (!signal.aborted) onLoadingChange?.(false);
    }
  }, [activeTab, client, granularity, onLoadingChange]);

  useEffect(() => {
    const current: QuerySession = {
      controller: new AbortController(),
      filters: resolveFilterWindow(filters),
      loadingMore: false,
    };
    session.current = current;
    void execute(current);
    return () => {
      current.controller.abort();
      onLoadingChange?.(false);
    };
  }, [execute, filters, onLoadingChange, refreshRevision, reloadRevision]);

  const loadMore = useCallback(async () => {
    const current = session.current;
    const previous = current?.eventPage;
    if (!current || current.controller.signal.aborted || current.loadingMore || !previous?.hasMore || !previous.nextCursor) return;
    // Lock synchronously: scroll observers may request the same cursor before React rerenders.
    current.loadingMore = true;
    setState((value) => ({ ...value, loadingMore: true, error: undefined }));
    try {
      const page = await client.events({ filters: current.filters, cursor: previous.nextCursor, limit: 100 }, current.controller.signal);
      if (current.controller.signal.aborted) return;
      const eventPage = { ...page, events: appendStableEventPage(previous.events, page.events) };
      current.eventPage = eventPage;
      setState((value) => ({ ...value, eventPage }));
    } catch (cause) {
      if (!current.controller.signal.aborted && !isAbortError(cause)) {
        setState((value) => ({ ...value, error: { cause, messageKey: 'usage.error.load_more_failed' } }));
      }
    } finally {
      current.loadingMore = false;
      if (!current.controller.signal.aborted) setState((value) => ({ ...value, loadingMore: false }));
    }
  }, [client]);

  const exportEvents = useCallback(async (format: 'csv' | 'json') => {
    const current = session.current;
    if (!current || current.controller.signal.aborted) return;
    try {
      const blob = await client.exportEvents(current.filters, format, current.controller.signal);
      if (!current.controller.signal.aborted) downloadBlob(blob, `gateway-usage-events.${format}`);
    } catch (cause) {
      if (!current.controller.signal.aborted && !isAbortError(cause)) {
        setState((value) => ({ ...value, error: { cause, messageKey: 'errors.export_failed' } }));
      }
    }
  }, [client]);

  return { ...state, loadMore, exportEvents, reload: () => setReloadRevision((value) => value + 1) };
}
