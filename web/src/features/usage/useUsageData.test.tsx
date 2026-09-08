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
  function Probe({ filters, revision, authGeneration }: { filters: GatewayUsageFilters; revision: number; authGeneration: number }) {
    const query = useUsageData({ client, filters, activeTab: 'events', granularity: 'auto', authGeneration, refreshRevision: revision, onLoadingChange });
    return <>
      <output>{query.eventPage?.events.map(item => item.id).join(',')}</output>
      <span role="status">{query.loading ? 'loading' : query.refreshing ? 'refreshing' : query.loadingMore ? 'more' : 'idle'}</span>
      {query.error && <p role="alert" data-error="load">{query.error.messageKey}</p>}
      {query.loadMoreError && <p role="alert" data-error="more">{query.loadMoreError.messageKey}</p>}
      {query.exportError && <p role="alert" data-error="export">{query.exportError.messageKey}</p>}
      <button onClick={query.loadMore}>More</button>
      <button onClick={query.retryLoadMore}>Retry More</button>
      <button onClick={query.reload}>Reload</button>
      <button onClick={() => query.exportEvents('csv')}>Export</button>
      <button onClick={() => query.exportEvents('json')}>Export JSON</button>
      <button onClick={query.retryExport}>Retry Export</button>
    </>;
  }
  const render = async (filters = baseFilters, revision = 0, authGeneration = 0) => {
    await act(async () => root.render(<Probe filters={filters} revision={revision} authGeneration={authGeneration} />));
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

  it('retains current rows while a same-query refresh runs or fails, then replaces them on retry', async () => {
    const refresh = deferred<UsageEventPageViewModel>();
    const retry = deferred<UsageEventPageViewModel>();
    client.events.mockResolvedValueOnce(page('current')).mockReturnValueOnce(refresh.promise).mockReturnValueOnce(retry.promise);
    await render();
    await render(baseFilters, 1);
    expect(displayed()).toBe('current');
    expect(container.querySelector('[role="status"]')?.textContent).toBe('refreshing');
    await act(async () => refresh.reject(new Error('Refresh failed')));
    expect(displayed()).toBe('current');
    expect(container.querySelector('[data-error="load"]')?.textContent).toBe('usage.error.load_failed');
    await click('Reload');
    expect(displayed()).toBe('current');
    expect(container.querySelector('[role="status"]')?.textContent).toBe('refreshing');
    await act(async () => retry.resolve(page('retried')));
    expect(displayed()).toBe('retried');
    expect(container.querySelector('[role="alert"]')).toBeNull();
  });

  it('clears the previous identity immediately when the authentication generation changes', async () => {
    const changedIdentity = deferred<UsageEventPageViewModel>();
    client.events.mockResolvedValueOnce(page('key-a')).mockReturnValueOnce(changedIdentity.promise);
    await render();
    await render(baseFilters, 1, 1);
    expect(displayed()).toBe('');
    expect(container.querySelector('[role="status"]')?.textContent).toBe('loading');
    await act(async () => changedIdentity.reject(new Error('Unauthorized')));
    expect(displayed()).toBe('');
    expect(container.querySelector('[data-error="load"]')?.textContent).toBe('usage.error.load_failed');
  });

  it('keeps the published window and cursor after refresh failure while pausing old pagination', async () => {
    const refresh = deferred<UsageEventPageViewModel>();
    client.events.mockResolvedValueOnce(page('current', 'published-cursor'))
      .mockReturnValueOnce(refresh.promise)
      .mockResolvedValueOnce(page('next'));
    client.exportEvents.mockRejectedValueOnce(new Error('Synthetic export failure'));
    await render();
    const publishedFilters = client.events.mock.calls[0][0].filters;
    vi.setSystemTime(new Date('2026-09-08T10:00:00Z'));
    await render(baseFilters, 1);
    const pendingFilters = client.events.mock.calls[1][0].filters;
    expect(pendingFilters).not.toEqual(publishedFilters);
    await click('More');
    expect(client.events).toHaveBeenCalledTimes(2);
    await act(async () => refresh.reject(new Error('Refresh failed')));
    expect(displayed()).toBe('current');
    await click('Export');
    expect(client.exportEvents.mock.calls[0][0]).toEqual(publishedFilters);
    await click('More');
    expect(client.events.mock.calls[2][0]).toMatchObject({
      filters: publishedFilters,
      cursor: 'published-cursor',
    });
    expect(displayed()).toBe('current,next');
  });

  it('clears an old query scope immediately and refuses its cursor until the new first page succeeds', async () => {
    const nextScope = deferred<UsageEventPageViewModel>();
    client.events.mockResolvedValueOnce(page('old', 'old-cursor')).mockReturnValueOnce(nextScope.promise);
    await render();
    await render({ ...baseFilters, provider: 'new-provider' });
    expect(displayed()).toBe('');
    expect(container.querySelector('[role="status"]')?.textContent).toBe('loading');
    await click('More');
    expect(client.events).toHaveBeenCalledTimes(2);
    expect(client.events.mock.calls[1][0]).not.toHaveProperty('cursor');
    await act(async () => nextScope.resolve(page('new')));
    expect(displayed()).toBe('new');
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

  it('pauses a failed cursor until an explicit retry reuses it without duplicating rows', async () => {
    client.events.mockResolvedValueOnce(page('first', 'cursor'))
      .mockRejectedValueOnce(new Error('Page failed'))
      .mockResolvedValueOnce({ ...page('second'), events: [...page('first').events, ...page('second').events] });
    await render();
    await click('More');
    expect(displayed()).toBe('first');
    expect(container.querySelector('[data-error="more"]')?.textContent).toBe('usage.error.load_more_failed');
    await click('More');
    expect(client.events).toHaveBeenCalledTimes(2);
    await click('Retry More');
    expect(client.events.mock.calls[1][0].cursor).toBe('cursor');
    expect(client.events.mock.calls[2][0].cursor).toBe('cursor');
    expect(displayed()).toBe('first,second');
    expect(container.querySelector('[data-error="more"]')).toBeNull();
  });

  it('retries only the failed export format without reloading the event list', async () => {
    client.events.mockResolvedValueOnce(page('first'));
    client.exportEvents.mockRejectedValueOnce(new Error('Export failed')).mockRejectedValueOnce(new Error('Export failed again'));
    await render();
    await click('Export');
    expect(container.querySelector('[data-error="export"]')?.textContent).toBe('errors.export_failed');
    await click('Retry Export');
    expect(client.exportEvents).toHaveBeenCalledTimes(2);
    expect(client.exportEvents.mock.calls.map((call) => call[1])).toEqual(['csv', 'csv']);
    expect(client.exportEvents.mock.calls[1][0]).toEqual(client.exportEvents.mock.calls[0][0]);
    expect(client.events).toHaveBeenCalledOnce();
  });

  it('downloads successful CSV and JSON exports without reloading the event list', async () => {
    const downloads: string[] = [];
    const createObjectURL = vi.spyOn(URL, 'createObjectURL')
      .mockReturnValueOnce('blob:synthetic-csv')
      .mockReturnValueOnce('blob:synthetic-json');
    const revokeObjectURL = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
    vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
      downloads.push(this.download);
    });
    client.events.mockResolvedValueOnce(page('first'));
    client.exportEvents.mockResolvedValueOnce(new Blob(['csv'])).mockResolvedValueOnce(new Blob(['json']));
    await render();
    await click('Export');
    await click('Export JSON');
    expect(client.exportEvents.mock.calls.map((call) => call[1])).toEqual(['csv', 'json']);
    expect(downloads).toEqual(['gateway-usage-events.csv', 'gateway-usage-events.json']);
    expect(createObjectURL).toHaveBeenCalledTimes(2);
    expect(revokeObjectURL.mock.calls.map((call) => call[0])).toEqual(['blob:synthetic-csv', 'blob:synthetic-json']);
    expect(client.events).toHaveBeenCalledOnce();
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
      const query = useUsageData({ client: realClient, filters, activeTab, granularity: 'auto', authGeneration: 0, refreshRevision: revision });
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
