// @vitest-environment happy-dom
import type { AdminTransport } from '@/admin-api/client';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { GatewayUsageClient } from '@/gateway-usage/client';
import { adaptUsageEventPage } from '@/gateway-usage/adapter';
import { gatewayUsageEventsFixture } from '@/test/fixtures/usage';
import type { GatewayUsageFilters, UsageEventPageViewModel } from '@/gateway-usage/types';
import { useUsageData } from './useUsageData';

const baseFilters: GatewayUsageFilters = {
  from: '2020-01-01T00:00:00Z', to: '2020-01-02T00:00:00Z',
  timeMode: 'relative', relativePreset: '24h',
};
const event = adaptUsageEventPage(gatewayUsageEventsFixture).events[0];
const page = (id: string, cursor?: string): UsageEventPageViewModel => ({
  events: [{ ...event, id, requestId: id }], nextCursor: cursor, hasMore: Boolean(cursor),
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

describe('usage query sessions', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const client = {
    events: vi.fn<GatewayUsageClient['events']>(),
    overview: vi.fn<GatewayUsageClient['overview']>(),
    summary: vi.fn<GatewayUsageClient['summary']>(),
    breakdown: vi.fn<GatewayUsageClient['breakdown']>(),
    exportEvents: vi.fn<GatewayUsageClient['exportEvents']>(),
  };
  const onLoadingChange = vi.fn();
  function Probe({ filters, revision }: { filters: GatewayUsageFilters; revision: number }) {
    const query = useUsageData({ client, filters, activeTab: 'events', granularity: 'auto', refreshRevision: revision, onLoadingChange });
    return <>
      <output>{query.eventPage?.events.map(item => item.id).join(',')}</output>
      <span role="status">{query.loading ? 'loading' : query.loadingMore ? 'more' : 'idle'}</span>
      {query.error && <p role="alert">{query.error.messageKey}</p>}
      <button onClick={query.loadMore}>More</button>
      <button onClick={query.reload}>Reload</button>
      <button onClick={() => query.exportEvents('csv')}>Export</button>
    </>;
  }
  const render = async (filters = baseFilters, revision = 0) => {
    await act(async () => root.render(<Probe filters={filters} revision={revision} />));
  };
  const click = async (label: string) => {
    await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === label)!.click());
  };
  const displayed = () => container.querySelector('output')?.textContent;

  beforeEach(() => {
    vi.resetAllMocks();
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(new Date('2026-09-08T08:00:00Z'));
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('freezes the rolling time range for pagination and export, recalculating it on refresh', async () => {
    client.events.mockResolvedValueOnce(page('first', 'cursor-1')).mockResolvedValueOnce(page('second')).mockResolvedValueOnce(page('refreshed'));
    await render();
    const first = client.events.mock.calls[0][0].filters;
    vi.setSystemTime(new Date('2026-09-08T10:00:00Z'));
    await click('More');
    expect(client.events.mock.calls[1][0]).toMatchObject({ filters: first, cursor: 'cursor-1' });
    client.exportEvents.mockRejectedValue(new Error('Export denied'));
    await click('Export');
    expect(client.exportEvents.mock.calls[0][0]).toEqual(first);
    await render(baseFilters, 1);
    expect(client.events.mock.calls[2][0].filters.to).toBe('2026-09-08T10:00:00.000Z');
  });

  it('discards the old first page even when the transport ignores abort', async () => {
    const old = deferred<UsageEventPageViewModel>();
    client.events.mockReturnValueOnce(old.promise).mockResolvedValueOnce(page('new'));
    await render();
    const oldSignal = client.events.mock.calls[0][1]!;
    await render({ ...baseFilters, provider: 'new-provider' });
    expect(oldSignal.aborted).toBe(true);
    await act(async () => old.resolve(page('obsolete')));
    expect(displayed()).toBe('new');
    expect(onLoadingChange).toHaveBeenLastCalledWith(false);
  });

  it('does not append an obsolete cursor page after filters change', async () => {
    const old = deferred<UsageEventPageViewModel>();
    client.events.mockResolvedValueOnce(page('old-first', 'old-cursor')).mockReturnValueOnce(old.promise).mockResolvedValueOnce(page('new-first'));
    await render();
    await click('More');
    const oldSignal = client.events.mock.calls[1][1]!;
    await render({ ...baseFilters, logicalModel: 'new-model' });
    await act(async () => old.resolve(page('old-second')));
    expect(oldSignal.aborted).toBe(true);
    expect(displayed()).toBe('new-first');
    expect(container.querySelector('[role="status"]')?.textContent).toBe('idle');
  });

  it('blocks duplicate concurrent cursor loads and removes page overlap', async () => {
    const next = deferred<UsageEventPageViewModel>();
    client.events.mockResolvedValueOnce(page('first', 'cursor')).mockReturnValueOnce(next.promise);
    await render();
    await act(async () => { container.querySelector('button')!.click(); container.querySelector('button')!.click(); });
    expect(client.events).toHaveBeenCalledTimes(2);
    await act(async () => next.resolve({ events: [...page('first').events, ...page('second').events], hasMore: false }));
    expect(displayed()).toBe('first,second');
  });

  it('starts a new cancellable session on retry and ignores failures from the old one', async () => {
    const old = deferred<UsageEventPageViewModel>();
    client.events.mockResolvedValueOnce(page('first', 'cursor')).mockReturnValueOnce(old.promise).mockResolvedValueOnce(page('retried'));
    await render();
    await click('More');
    await click('Reload');
    await act(async () => old.reject(new Error('Old network failure')));
    expect(displayed()).toBe('retried');
    expect(container.querySelector('[role="alert"]')).toBeNull();
    expect(client.events.mock.calls[1][1]!.aborted).toBe(true);
  });

  it('aborts requests and clears the shell busy state when the page unmounts', async () => {
    const pending = deferred<UsageEventPageViewModel>();
    client.events.mockReturnValueOnce(pending.promise);
    await render();
    await act(async () => root.render(null));
    expect(client.events.mock.calls[0][1]!.aborted).toBe(true);
    expect(onLoadingChange).toHaveBeenLastCalledWith(false);
    await act(async () => pending.resolve(page('too-late')));
    expect(container.textContent).toBe('');
  });
});

// Exercise the actual client join through the hook, including transports that
// ignore cancellation. A late usage_source response must never cross sessions.
describe('summary source query sessions', () => {
  it.each(['overview', 'analysis'] as const)('keeps %s source counts within the current filter/refresh session', async (activeTab) => {
    const { GatewayUsageClient } = await import('@/gateway-usage/client');
    const { gatewayUsageSummaryFixture } = await import('@/test/fixtures/usage');
    const oldSources = deferred<unknown>();
    let sourceCalls = 0;
    const json = vi.fn(async (url: string) => {
      if (url.includes('/summary')) return gatewayUsageSummaryFixture;
      if (url.includes('breakdown=usage_source')) {
        sourceCalls++;
        return sourceCalls === 1 ? oldSources.promise : { data: [{ key: 'estimated', logical_requests: sourceCalls }] };
      }
      return { data: [] };
    });
    const realClient = new GatewayUsageClient({ json: json as AdminTransport['json'], blob: vi.fn() });
    const host = document.createElement('div');
    const root = createRoot(host);
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    function SummaryProbe({ filters, revision }: { filters: GatewayUsageFilters; revision: number }) {
      const query = useUsageData({ client: realClient, filters, activeTab, granularity: 'auto', refreshRevision: revision });
      return <output>{JSON.stringify(query.overview?.summary.usageSources ?? query.analysisSummary?.usageSources)}</output>;
    }
    try {
      await act(async () => root.render(<SummaryProbe filters={baseFilters} revision={0} />));
      expect(host.textContent).toBe('');
      const changed = { ...baseFilters, sourceId: 'new-source' };
      await act(async () => root.render(<SummaryProbe filters={changed} revision={0} />));
      expect(host.textContent).toBe('{"estimated":2}');
      await act(async () => root.render(<SummaryProbe filters={changed} revision={1} />));
      expect(host.textContent).toBe('{"estimated":3}');
      await act(async () => oldSources.resolve({ data: [{ key: 'missing', logical_requests: 99 }] }));
      expect(host.textContent).toBe('{"estimated":3}');
      expect(sourceCalls).toBe(3);
      const calls = json.mock.calls as unknown as [string, { signal: AbortSignal }][];
      expect(calls[0][1].signal.aborted).toBe(true);
    } finally { act(() => root.unmount()); }
  });
});
