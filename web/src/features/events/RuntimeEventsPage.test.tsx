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
    window.location.hash = '';
    vi.unstubAllGlobals();
  });

  const getAdminKey = () => 'test-admin-key';
  const onLoadingChange = () => {};
  const renderPage = async (refreshRevision = 0) => {
    await act(async () => {
      root.render(
        <GatewayManagementPage
          page="runtime-events"
          getAdminKey={getAdminKey}
          adminKeyConfigured
          clearAdminKey={() => {}}
          refreshRevision={refreshRevision}
          onLoadingChange={onLoadingChange}
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
    expect(container.textContent).toContain('数据库连接失败');
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

  it('locks cursor requests synchronously and removes overlapping events', async () => {
    let finish!: (value: Response) => void;
    const fetchMock = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify(response([event], true, 'cursor-1'))))
      .mockImplementationOnce(() => new Promise<Response>((resolve) => { finish = resolve; }));
    vi.stubGlobal('fetch', fetchMock);
    await renderPage();
    const more = [...container.querySelectorAll('button')].find((button) => button.textContent === '加载更多')!;
    await act(async () => { more.click(); more.click(); });
    expect(fetchMock).toHaveBeenCalledTimes(2);
    await act(async () => finish(new Response(JSON.stringify(response([event, { ...event, event_id: 'system:2' }])))));
    expect(container.querySelectorAll('tbody tr')).toHaveLength(2);
  });

  it('cancels pagination when refresh starts and keeps the published cursor after refresh fails', async () => {
    let finishOldPage!: (value: Response) => void;
    let failRefresh!: (reason: Error) => void;
    const initial = new Response(JSON.stringify(response([event], true, 'cursor-1')));
    const second = { ...event, event_id: 'system:2', message: 'Second event' };
    const fetchMock = vi.fn<typeof fetch>()
      .mockResolvedValueOnce(initial)
      .mockResolvedValueOnce(new Response(JSON.stringify(response([second], true, 'cursor-2'))))
      .mockImplementationOnce(() => new Promise<Response>((resolve) => { finishOldPage = resolve; }))
      .mockImplementationOnce(() => new Promise<Response>((_resolve, reject) => { failRefresh = reject; }))
      .mockResolvedValueOnce(new Response(JSON.stringify(response([{ ...event, event_id: 'system:3' }]))))
      .mockResolvedValueOnce(new Response(JSON.stringify(response([{ ...event, event_id: 'system:new', message: 'New first page' }]))));
    vi.stubGlobal('fetch', fetchMock);
    await renderPage();
    const more = () => [...container.querySelectorAll('button')].find((button) => button.textContent === '加载更多')!;
    await act(async () => more().click());
    await waitFor(() => container.querySelectorAll('tbody tr').length === 2);
    await act(async () => more().click());
    const oldSignal = fetchMock.mock.calls[2][1]!.signal!;
    await renderPage(1);
    expect(oldSignal.aborted).toBe(true);
    expect(container.querySelectorAll('tbody tr')).toHaveLength(2);
    await act(async () => more().click());
    expect(fetchMock).toHaveBeenCalledTimes(4);
    await act(async () => finishOldPage(new Response(JSON.stringify(response([{ ...event, event_id: 'system:old', message: 'Obsolete page' }])))));
    expect(container.textContent).not.toContain('Obsolete page');
    await act(async () => failRefresh(new Error('Refresh failed')));
    expect(container.querySelectorAll('tbody tr')).toHaveLength(2);
    await act(async () => more().click());
    expect(String(fetchMock.mock.calls[4][0])).toContain('cursor=cursor-2');
    await waitFor(() => container.querySelectorAll('tbody tr').length === 3);
    await renderPage(2);
    await waitFor(() => container.textContent?.includes('New first page') ?? false);
    expect(container.querySelectorAll('tbody tr')).toHaveLength(1);
    expect(container.textContent).not.toContain('Second event');
  });

  it('keeps filters available after a failed query, validates the time range, and recovers after reset', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes('category=database')) {
        return new Response(JSON.stringify({ error: { code: 'events_query_failed', message: 'Synthetic failure' } }), { status: 503 });
      }
      return new Response(JSON.stringify(response([event])), { status: 200 });
    });
    vi.stubGlobal('fetch', fetchMock);
    await renderPage();
    await waitFor(() => container.textContent?.includes('数据库连接失败') ?? false);

    const input = (label: string) => {
      const id = [...container.querySelectorAll('label')].find((item) => item.textContent === label)!.htmlFor;
      return document.getElementById(id) as HTMLInputElement;
    };
    const setValue = (element: HTMLInputElement, value: string) => act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(element, value);
      element.dispatchEvent(new Event('input', { bubbles: true }));
    });
    setValue(input('起始时间（本地）'), '2026-09-10T12:00');
    setValue(input('结束时间（本地）'), '2026-09-10T11:00');
    const apply = [...container.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent === '应用')!;
    const requestsBeforeInvalidApply = fetchMock.mock.calls.length;
    await act(async () => apply.click());
    expect(fetchMock).toHaveBeenCalledTimes(requestsBeforeInvalidApply);
    expect(input('结束时间（本地）').getAttribute('aria-invalid')).toBe('true');
    expect(container.textContent).toContain('结束时间必须晚于开始时间');

    setValue(input('起始时间（本地）'), '');
    setValue(input('结束时间（本地）'), '');
    await selectComboboxValue(input('分类'), 'database');
    await act(async () => apply.click());
    await waitFor(() => container.textContent?.includes('events_query_failed') ?? false);
    expect(container.querySelector('section[aria-label="运行事件筛选"]')).not.toBeNull();
    expect([...container.querySelectorAll('button')].some((button) => button.textContent === '重置')).toBe(true);

    await act(async () => [...container.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent === '重置')!.click());
    await waitFor(() => !container.textContent?.includes('events_query_failed'));
    expect(container.textContent).toContain('数据库连接失败');
  });

  it('shows request failure details, copies a full ID, and links to its request, source, and account', async () => {
    const request = {
      ...event, event_id: 'request:hashed', event_type: 'request.failed', category: 'request',
      subject_type: 'request', subject_id: 'req/abc', correlation_id: 'req/abc', source: 'usage_events',
      message: 'Gateway request failed',
      details: { error_summary: 'Upstream timed out', status_code: 504, logical_model: 'gpt-test', source_id: 'source/one', account_id: 'account one' },
    };
    const fetchMock = vi.fn(async () => new Response(JSON.stringify(response([request]))));
    vi.stubGlobal('fetch', fetchMock);
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
    await renderPage();
    expect(container.textContent).toContain('请求失败');
    expect(container.textContent).toContain('Upstream timed out');
    expect(container.textContent).toContain('HTTP 504');
    expect(container.textContent).toContain('gpt-test');
    expect(container.querySelectorAll('thead th')).toHaveLength(6);
    expect(container.querySelector('tbody')?.textContent).not.toContain('req/abc');
    await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="查看事件 request:hashed"]')!.click());
    const buttons = [...document.body.querySelectorAll<HTMLButtonElement>('button')];
    await act(async () => buttons.find((button) => button.textContent === '复制 ID')!.click());
    expect(writeText).toHaveBeenCalledWith('request:hashed');
    expect(document.body.textContent).toContain('已复制完整 ID');
    expect(buttons.some((button) => button.textContent === '查看同关联事件')).toBe(true);
    expect(buttons.some((button) => button.textContent === '查看来源详情')).toBe(true);
    expect(buttons.some((button) => button.textContent === '查看账号额度')).toBe(true);
    await act(async () => buttons.find((button) => button.textContent === '查看来源详情')!.click());
    expect(window.location.hash).toBe('#sources/source%2Fone');
    await act(async () => buttons.find((button) => button.textContent === '查看账号额度')!.click());
    expect(window.location.hash).toBe('#upstream-quotas/account%20one');
    await act(async () => buttons.find((button) => button.textContent === '查看请求详情')!.click());
    expect(window.location.hash).toBe('#events?request_id=req%2Fabc');
  });

  it('filters related events and preserves unknown event types in the list', async () => {
    const unknown = { ...event, event_type: 'vendor.new_failure', message: 'Specific issue' };
    const fetchMock = vi.fn(async (_input: RequestInfo | URL) => new Response(JSON.stringify(response([unknown]))));
    vi.stubGlobal('fetch', fetchMock);
    await renderPage();
    expect(container.querySelector('tbody')?.textContent).toContain('vendor.new_failure');
    expect(container.querySelector('tbody')?.textContent).toContain('Specific issue');
    await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="查看事件 system:1"]')!.click());
    await act(async () => [...document.body.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent === '查看同关联事件')!.click());
    await waitFor(() => fetchMock.mock.calls.some(([url]) => String(url).includes('correlation_id=incident-one')));
  });
});
