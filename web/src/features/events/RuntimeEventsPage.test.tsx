// @vitest-environment happy-dom
import { GatewayManagementPage } from '@/pages/GatewayManagementPage';
import { selectComboboxValue } from '@/test/interactions';
import { createRoot } from '@/test/render';
import { setTestLanguage } from '@/test/setup';
import { act } from 'react';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

const event = {
  event_id: 'system:1',
  occurred_at: '2026-09-09T01:02:03Z',
  category: 'database',
  event_type: 'database.connection_failed',
  level: 'error',
  subject_type: 'database',
  subject_id: 'postgresql',
  correlation_id: 'incident-one',
  message: 'Database operation became unavailable',
  details: { component: 'events.query', error_code: 'database_unavailable' },
  source: 'system_events',
} as const;

const response = (data: unknown[], hasMore = false, nextCursor: string | null = null) => ({
  version: 'v1',
  timezone: 'UTC',
  fact_source: 'postgresql_unified_read_model',
  range: { from: null, since: null, to: null, boundary: '[from,to)' },
  data,
  page: { limit: 100, has_more: hasMore, next_cursor: nextCursor },
});

async function waitFor(condition: () => boolean) {
  const deadline = Date.now() + 1000;
  while (!condition() && Date.now() < deadline) {
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)); });
  }
  expect(condition()).toBe(true);
}

describe('RuntimeEventsPage', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeAll(() => setTestLanguage('zh'));

  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  const renderPage = async () => {
    await act(async () => {
      root.render(
        <GatewayManagementPage
          page="runtime-events"
          getAdminKey={() => 'test-admin-key'}
          adminKeyConfigured
          clearAdminKey={() => {}}
          refreshRevision={0}
          onLoadingChange={() => {}}
        />,
      );
    });
    await waitFor(() => container.querySelector('[data-od-id="page-runtime-events"]') !== null);
  };

  it('loads cursor pages and exposes only sanitized event metadata in the details drawer', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes('cursor=')) {
        return new Response(JSON.stringify(response([{
          ...event,
          event_id: 'audit:2',
          category: 'operation',
          event_type: 'retention.cleanup.completed',
          level: 'info',
          message: 'retention.cleanup.completed',
          source: 'audit_logs',
        }])), { status: 200 });
      }
      return new Response(JSON.stringify(response([event], true, '123:system:1')), { status: 200 });
    });
    vi.stubGlobal('fetch', fetchMock);
    await renderPage();

    expect(fetchMock).toHaveBeenCalledWith('/admin/events?limit=100', expect.any(Object));
    expect(container.textContent).toContain('database.connection_failed');
    const loadMore = [...container.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent === '加载更多')!;
    await act(async () => loadMore.click());
    await waitFor(() => container.textContent?.includes('retention.cleanup.completed') ?? false);
    expect(fetchMock).toHaveBeenCalledWith(
      '/admin/events?limit=100&cursor=123%3Asystem%3A1',
      expect.any(Object),
    );

    await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="查看事件 system:1"]')!.click());
    expect(document.body.textContent).toContain('database_unavailable');
    expect(document.body.textContent).toContain('events.query');
    expect(document.body.textContent).not.toContain('Authorization');
  });

  it('keeps draft filters local until apply and sends the selected unified dimensions', async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify(response([event])), { status: 200 }));
    vi.stubGlobal('fetch', fetchMock);
    await renderPage();
    expect(fetchMock).toHaveBeenCalledTimes(1);

    const categoryLabel = [...container.querySelectorAll('label')]
      .find((label) => label.textContent === '分类')!;
    const category = document.getElementById(categoryLabel.htmlFor) as HTMLInputElement;
    await selectComboboxValue(category, 'database');
    expect(fetchMock).toHaveBeenCalledTimes(1);

    const apply = [...container.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent === '应用')!;
    await act(async () => apply.click());
    await waitFor(() => fetchMock.mock.calls.length === 2);
    expect(fetchMock).toHaveBeenLastCalledWith(
      '/admin/events?category=database&limit=100',
      expect.any(Object),
    );
  });

  it('loads event types remotely and applies selection only after Apply', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.startsWith('/admin/events/filter-options')) {
        return new Response(JSON.stringify({ data: ['request.failed', 'source.discovery.failed'], has_more: false }));
      }
      return new Response(JSON.stringify(response([event])));
    });
    vi.stubGlobal('fetch', fetchMock);
    await renderPage();
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const id = [...container.querySelectorAll('label')].find((label) => label.textContent === '事件类型')!.htmlFor;
    const input = document.getElementById(id) as HTMLInputElement;
    act(() => input.click());
    await waitFor(() => container.querySelector('[role="option"][value="request.failed"]') !== null);
    await selectComboboxValue(input, 'request.failed');
    expect(input.value).toBe('request.failed');
    expect(fetchMock.mock.calls.filter(([url]) => String(url).startsWith('/admin/events?'))).toHaveLength(1);
    await act(async () => [...container.querySelectorAll('button')].find((button) => button.textContent === '应用')!.click());
    await waitFor(() => fetchMock.mock.calls.some(([url]) => String(url).includes('event_type=request.failed')));
    expect(fetchMock).toHaveBeenCalledWith('/admin/events?event_type=request.failed&limit=100', expect.any(Object));
  });

  it('cancels an old cursor request when new filters are applied', async () => {
    let cursorSignal: AbortSignal | undefined;
    let finishCursor: (() => void) | undefined;
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url.includes('cursor=')) {
        cursorSignal = init?.signal ?? undefined;
        return new Promise<Response>((resolve) => {
          finishCursor = () => resolve(new Response(JSON.stringify(response([])), { status: 200 }));
        });
      }
      const initial = !url.includes('category=');
      return Promise.resolve(new Response(
        JSON.stringify(response([event], initial, initial ? '123:system:1' : null)),
        { status: 200 },
      ));
    });
    vi.stubGlobal('fetch', fetchMock);
    await renderPage();

    const loadMore = [...container.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent === '加载更多')!;
    await act(async () => loadMore.click());
    await waitFor(() => cursorSignal !== undefined);

    const categoryLabel = [...container.querySelectorAll('label')]
      .find((label) => label.textContent === '分类')!;
    const category = document.getElementById(categoryLabel.htmlFor) as HTMLInputElement;
    await selectComboboxValue(category, 'database');
    const apply = [...container.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent === '应用')!;
    await act(async () => apply.click());
    await waitFor(() => fetchMock.mock.calls.some(([input]) => String(input).includes('category=database')));

    expect(cursorSignal?.aborted).toBe(true);
    finishCursor?.();
    await act(async () => Promise.resolve());
  });
});
