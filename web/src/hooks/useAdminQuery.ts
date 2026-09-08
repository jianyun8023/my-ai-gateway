import type { AdminErrorShape } from '@/admin-api';
import { isAbortError, normalizeAdminError } from '@/admin-api';
import { useCallback, useEffect, useState } from 'react';

interface AdminQueryState<T> {
  queryKey: string;
  data?: T;
  loading: boolean;
  refreshing: boolean;
  error?: AdminErrorShape;
}

interface UseAdminQueryOptions<T> {
  load: (signal: AbortSignal) => Promise<T>;
  queryKey?: string;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
}

export function useAdminQuery<T>({
  load,
  queryKey = '',
  refreshRevision = 0,
  onBusyChange,
}: UseAdminQueryOptions<T>) {
  const [reloadRevision, setReloadRevision] = useState(0);
  const [state, setState] = useState<AdminQueryState<T>>({
    queryKey,
    loading: true,
    refreshing: false,
  });

  const execute = useCallback(async (signal: AbortSignal, requestKey: string) => {
    setState((current) => ({
      ...(current.queryKey === requestKey ? current : { data: undefined }),
      queryKey: requestKey,
      loading: current.queryKey !== requestKey || current.data === undefined,
      refreshing: current.queryKey === requestKey && current.data !== undefined,
      error: undefined,
    }));
    onBusyChange?.(true);
    try {
      const data = await load(signal);
      if (!signal.aborted) {
        setState({ queryKey: requestKey, data, loading: false, refreshing: false });
      }
    } catch (error) {
      if (!signal.aborted && !isAbortError(error)) {
        setState((current) => ({
          ...current,
          loading: false,
          refreshing: false,
          error: normalizeAdminError(error),
        }));
      }
    } finally {
      if (!signal.aborted) onBusyChange?.(false);
    }
  }, [load, onBusyChange]);

  useEffect(() => {
    const controller = new AbortController();
    void execute(controller.signal, queryKey);
    return () => {
      controller.abort();
      onBusyChange?.(false);
    };
  }, [execute, onBusyChange, queryKey, refreshRevision, reloadRevision]);

  const visibleState = state.queryKey === queryKey
    ? state
    : { queryKey, loading: true, refreshing: false };

  return {
    ...visibleState,
    reload: () => setReloadRevision((current) => current + 1),
  };
}
