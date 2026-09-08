import type { AdminErrorShape } from '@/admin-api';
import { isAbortError, normalizeAdminError } from '@/admin-api';
import { useCallback, useEffect, useState } from 'react';

interface AdminQueryState<T> {
  data?: T;
  loading: boolean;
  refreshing: boolean;
  error?: AdminErrorShape;
}

interface UseAdminQueryOptions<T> {
  load: (signal: AbortSignal) => Promise<T>;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
}

export function useAdminQuery<T>({
  load,
  refreshRevision = 0,
  onBusyChange,
}: UseAdminQueryOptions<T>) {
  const [reloadRevision, setReloadRevision] = useState(0);
  const [state, setState] = useState<AdminQueryState<T>>({
    loading: true,
    refreshing: false,
  });

  const execute = useCallback(async (signal: AbortSignal) => {
    setState((current) => ({
      ...current,
      loading: current.data === undefined,
      refreshing: current.data !== undefined,
      error: undefined,
    }));
    onBusyChange?.(true);
    try {
      const data = await load(signal);
      if (!signal.aborted) {
        setState({ data, loading: false, refreshing: false });
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
    void execute(controller.signal);
    return () => {
      controller.abort();
      onBusyChange?.(false);
    };
  }, [execute, onBusyChange, refreshRevision, reloadRevision]);

  return {
    ...state,
    reload: () => setReloadRevision((current) => current + 1),
  };
}
