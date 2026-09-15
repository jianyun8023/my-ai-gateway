import { isAbortError } from '@/admin-api/errors';
import { useCallback, useEffect, useRef, useState } from 'react';

export interface QuerySession<T> {
  readonly data: T;
  readonly signal: AbortSignal;
  isCurrent: () => boolean;
}

interface PublishedSession<T> extends QuerySession<T> {
  queryKey: string;
  controller: AbortController;
}

interface QueryState<T> {
  queryKey: string;
  session?: QuerySession<T>;
  loading: boolean;
  refreshing: boolean;
  error?: unknown;
}

export interface QuerySessionOptions<T> {
  load: (signal: AbortSignal) => Promise<T>;
  queryKey?: string;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
}

// First-page results own the lifetime of their follow-up requests. Refreshing
// cancels old work but retains the published data/window until a new load wins.
export function useQuerySession<T>({ load, queryKey = '', refreshRevision = 0, onBusyChange }: QuerySessionOptions<T>) {
  const [reloadRevision, setReloadRevision] = useState(0);
  const [state, setState] = useState<QueryState<T>>({ queryKey, loading: true, refreshing: false });
  const published = useRef<PublishedSession<T> | undefined>(undefined);

  useEffect(() => {
    const publish = (data: T): PublishedSession<T> => {
      published.current?.controller.abort();
      const controller = new AbortController();
      const session: PublishedSession<T> = {
        queryKey, data, controller, signal: controller.signal,
        isCurrent: () => !controller.signal.aborted && published.current === session,
      };
      published.current = session;
      return session;
    };
    const previous = published.current;
    previous?.controller.abort();
    const retained = previous?.queryKey === queryKey ? publish(previous.data) : undefined;
    published.current = retained;
    setState({ queryKey, session: retained, loading: !retained, refreshing: Boolean(retained) });
    const controller = new AbortController();
    onBusyChange?.(true);
    const execute = async () => {
      try {
        const data = await load(controller.signal);
        // A transport may resolve after abort; it still cannot publish results.
        if (!controller.signal.aborted) {
          setState({ queryKey, session: publish(data), loading: false, refreshing: false });
        }
      } catch (error) {
        if (!controller.signal.aborted && !isAbortError(error)) {
          setState({ queryKey, session: retained, loading: false, refreshing: false, error });
        }
      } finally {
        if (!controller.signal.aborted) onBusyChange?.(false);
      }
    };
    void execute();
    return () => {
      controller.abort();
      published.current?.controller.abort();
      onBusyChange?.(false);
    };
  }, [load, onBusyChange, queryKey, refreshRevision, reloadRevision]);

  const getSession = useCallback((): QuerySession<T> | undefined => {
    const session = published.current;
    return session?.queryKey === queryKey && session.isCurrent() ? session : undefined;
  }, [queryKey]);
  const reload = useCallback(() => setReloadRevision((value) => value + 1), []);
  const visible = state.queryKey === queryKey ? state : { queryKey, loading: true, refreshing: false };
  return { ...visible, data: visible.session?.data, getSession, reload };
}
