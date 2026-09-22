// @vitest-environment happy-dom
import { act, useState } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createRoot } from '@/test/render';
import { setTestLanguage } from '@/test/setup';
import { gatewayUsageEventsFixture } from '@/test/fixtures/usage';
import { adaptUsageEventPage } from '@/gateway-usage/adapter';
import { AdminClient } from '@/admin-api/client';
import { GatewayUsageClient } from '@/gateway-usage';
import { EVENT_COLUMNS, type EventColumn } from './eventColumns';
import { CacheBreakdown, TokenBreakdown, TpsBreakdown } from './EventMetricCells';
import { EventsTable } from './UsageEvents';
import { formatEventTime, formatTime } from './formatters';
import type { UsageEventViewModel } from '@/gateway-usage';

describe('event column preferences', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  beforeEach(async () => {
    await setTestLanguage('zh');
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); });

  it('changes visible columns and returns focus after dismissing the preferences', async () => {
    const onChange = vi.fn();
    const fetchImpl = vi.fn<typeof fetch>();
    const client = new GatewayUsageClient(new AdminClient({ fetchImpl }));
    function Page() {
      const [columns, setColumns] = useState<EventColumn[]>([...EVENT_COLUMNS]);
      return <EventsTable events={adaptUsageEventPage(gatewayUsageEventsFixture).events} hasMore={false} loadingMore={false} onLoadMore={() => {}}
        visibleColumns={columns} onVisibleColumnsChange={(next) => { onChange(next); setColumns(next); }} onExport={() => {}} client={client} />;
    }
    await act(async () => root.render(<Page />));
    const trigger = [...container.querySelectorAll<HTMLButtonElement>('button')].find(b => b.textContent === '列偏好')!;
    await act(async () => { trigger.focus(); trigger.click(); });
    const checkbox = container.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    expect(checkbox.checked).toBe(true);
    act(() => checkbox.click());
    expect(onChange).toHaveBeenCalledOnce();
    expect(onChange.mock.calls[0][0]).not.toContain('time');
    act(() => checkbox.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
    expect(container.querySelector('[role="dialog"]')).toBeNull();
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 30)); });
    expect(document.activeElement).toBe(trigger);
    expect(fetchImpl).not.toHaveBeenCalled();
  });
});

describe('event metric cells', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const fixtureEvents = adaptUsageEventPage(gatewayUsageEventsFixture).events;
  const richEvent: UsageEventViewModel = {
    ...fixtureEvents[0],
    usageSource: 'parsed',
    tokens: { input: 23373, output: 299, reasoning: 143, cached: 22528, cacheRead: 22528, cacheCreation: 0, total: 23672 },
  };
  beforeEach(async () => {
    await setTestLanguage('zh');
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockImplementation(function (this: HTMLElement) {
      return this.tagName === 'TR' ? 74 : this.getAttribute('role') === 'region' ? 420 : 0;
    });
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); vi.restoreAllMocks(); });

  const renderTable = async (events: UsageEventViewModel[], columns: EventColumn[] = ['tokens', 'cache']) => {
    const client = new GatewayUsageClient(new AdminClient({ fetchImpl: vi.fn<typeof fetch>() }));
    await act(async () => root.render(<EventsTable events={events} hasMore={false} loadingMore={false} onLoadMore={() => {}}
      visibleColumns={columns} onVisibleColumnsChange={() => {}} onExport={() => {}} client={client} />));
  };

  it('distinguishes model mapping from fallback, including fallback to the same model', async () => {
    await renderTable([
      { ...richEvent, id: 'mapped', logicalModel: 'friendly-name', upstreamModel: 'actual-model', fallback: false, fallbackReason: undefined, retryCount: 0 },
      { ...richEvent, id: 'same', logicalModel: 'same-model', upstreamModel: 'same-model', fallback: true, fallbackReason: 'account_cooling_down', retryCount: 0 },
      { ...richEvent, id: 'retried', fallback: false, fallbackReason: undefined, retryCount: 2 },
    ], ['model', 'retries']);
    const rows = container.querySelectorAll('tbody tr');
    expect(rows[0].textContent).toContain('friendly-name');
    expect(rows[0].textContent).toContain('actual-model');
    expect(rows[0].querySelector('[aria-label*="已回退"]')).toBeNull();
    expect(rows[0].querySelectorAll('td')[2].textContent).toBe('—');
    expect(rows[1].textContent?.match(/same-model/g)).toHaveLength(1);
    expect(rows[1].querySelector('[aria-label*="已回退"]')?.getAttribute('title')).toContain('主账号冷却中');
    expect(rows[1].querySelectorAll('td')[2].textContent).toBe('已回退');
    expect(rows[2].querySelectorAll('td')[2].textContent).toBe('2');
  });

  it('labels known clients and preserves unknown client identities', async () => {
    await renderTable(['claude-code', 'codex', 'kimi_code', 'curl', 'custom-agent/v2'].map((clientSource) => ({
      ...richEvent, id: clientSource, clientSource,
    })), ['clientSource']);
    const clients = [...container.querySelectorAll('tbody tr')].map((row) => row.querySelectorAll('td')[1]);
    expect(clients.map((cell) => cell.textContent)).toEqual(['Claude Code', 'Codex', 'Kimi Code', 'curl', 'custom-agent/v2']);
    expect(clients[4].querySelector('[title]')?.getAttribute('title')).toBe('custom-agent/v2');
    expect(clients.every((cell) => cell.querySelector('svg'))).toBe(true);
  });

  it('keeps dates visible across years, with full local timestamps available', async () => {
    const dates = ['2025-12-31T12:00:00Z', '2026-01-01T12:00:00Z', 'invalid'];
    await renderTable(dates.map((createdAt) => ({ ...richEvent, id: createdAt, createdAt })), ['time']);
    const times = [...container.querySelectorAll('tbody time')];
    expect(times[0].getAttribute('title')).toBe(formatTime(dates[0]));
    expect(times[0].textContent).toContain(formatEventTime(dates[0]).time);
    expect(times[0].querySelector('small')?.textContent).toContain('2025');
    expect(times[1].querySelector('small')?.textContent).toContain('2026');
    expect(times[2].textContent).toBe('—');
  });

  it('preserves unknown and zero latency while scaling valid bars to loaded requests', async () => {
    await renderTable([undefined, 0, 5_000, 20_000, Number.NaN, -1].map((latencyMs, index) => ({
      ...richEvent, id: String(index), latencyMs,
    })), ['latency']);
    const cells = [...container.querySelectorAll('tbody tr')].map((row) => row.querySelectorAll('td')[1]);
    expect(cells.map((cell) => cell.textContent)).toEqual(['—', '0 ms', '5.0 s', '20 s', '—', '—']);
    expect(cells[0].querySelector('[data-latency-tone]')).toBeNull();
    expect(cells[1].querySelector<HTMLElement>('[data-latency-tone]')?.style.getPropertyValue('--latency-width')).toBe('0%');
    expect(cells[2].querySelector<HTMLElement>('[data-latency-tone="warning"]')?.style.getPropertyValue('--latency-width')).toBe('25%');
    expect(cells[3].querySelector('[title]')?.getAttribute('title')).toBe('20,000 ms');
    expect(cells[3].querySelector<HTMLElement>('[data-latency-tone="danger"]')?.style.getPropertyValue('--latency-width')).toBe('100%');
  });

  it('retains source warnings on successful missing-usage requests and reported zero on failures', async () => {
    await renderTable([
      { ...fixtureEvents[1], id: 'missing-success', success: true },
      { ...fixtureEvents[1], id: 'reported-failure', usageSource: 'upstream' },
    ], ['tokens']);
    const rows = container.querySelectorAll('tbody tr');
    expect(rows[0].textContent).toContain('未获取');
    expect(rows[1].querySelectorAll('td')[1].textContent).toBe('0');
  });

  it('renders compact token totals and cache hit rates in the row', async () => {
    await renderTable([richEvent, fixtureEvents[1]]);
    const rows = [...container.querySelectorAll('tbody tr')];
    const cellsOf = (row: Element) => [...row.querySelectorAll('td')].map((cell) => cell.textContent);
    expect(cellsOf(rows[0])[1]).toBe('23.7K');
    expect(cellsOf(rows[0])[2]).toBe('96.4%');
    // Failed requests with missing usage remain quiet, without claiming zero.
    expect(cellsOf(rows[1])[1]).toBe('—');
    expect(cellsOf(rows[1])[2]).toBe('—');
  });

  it('reveals the full token breakdown from the token button without opening the row', async () => {
    await renderTable([richEvent]);
    const tokenCell = container.querySelectorAll('tbody tr td')[1];
    const trigger = tokenCell.querySelector<HTMLButtonElement>('button')!;
    expect(trigger.type).toBe('button');
    expect(trigger.getAttribute('aria-label')).toContain('Token 用量详情');
    await act(async () => { trigger.focus(); trigger.click(); });
    expect(trigger.getAttribute('aria-expanded')).toBe('true');
    expect(document.querySelector('[data-od-id="event-drawer"]')).toBeNull();
    const popover = document.querySelector('[data-od-id="token-breakdown"]');
    expect(popover?.textContent).toContain('Token 用量详情');
    expect(popover?.textContent).toContain('23,373');
    expect(popover?.textContent).toContain('22,528');
    expect(popover?.textContent).toContain('23,672');
    expect(popover?.textContent).toContain('上游流式');
    await act(async () => { popover!.querySelector('dd')!.click(); });
    expect(document.querySelector('[data-od-id="event-drawer"]')).toBeNull();
  });

  it('closes metric details with Escape and outside click, then keeps normal row opening', async () => {
    await renderTable([richEvent]);
    const row = container.querySelector<HTMLTableRowElement>('tbody tr')!;
    const trigger = row.querySelectorAll<HTMLButtonElement>('td button')[1];
    await act(async () => { trigger.focus(); trigger.click(); });
    expect(trigger.getAttribute('aria-expanded')).toBe('true');
    expect(document.activeElement).toBe(trigger);
    await act(async () => {
      document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    });
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 30)); });
    expect(document.activeElement).toBe(trigger);

    await act(async () => { trigger.click(); });
    expect(trigger.getAttribute('aria-expanded')).toBe('true');
    const dropdown = document.querySelector<HTMLElement>('[data-od-id="token-breakdown"]')!.parentElement!;
    dropdown.tabIndex = -1;
    dropdown.focus();
    await act(async () => { document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })); });
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 30)); });
    expect(document.activeElement).toBe(trigger);
    expect(document.querySelector('[data-od-id="event-drawer"]')).toBeNull();

    await act(async () => { row.click(); });
    expect(document.querySelector('[data-od-id="event-drawer"]')).not.toBeNull();
  });

  it('reveals cache hit details from the cache button', async () => {
    await renderTable([richEvent]);
    const cacheCell = container.querySelectorAll('tbody tr td')[2];
    await act(async () => { cacheCell.querySelector<HTMLButtonElement>('button')!.click(); });
    const popover = document.querySelector('[data-od-id="cache-breakdown"]');
    expect(popover?.textContent).toContain('缓存详情');
    expect(popover?.textContent).toContain('96.4%');
    expect(popover?.textContent).toContain('22,528');
    expect(popover?.textContent).toContain('缓存创建不计入命中');
  });

  it('renders average output speed over total latency for streaming requests', async () => {
    // 299 output tokens over 10s total latency = 29.9 t/s, including first-data wait.
    const tpsEvent: UsageEventViewModel = { ...richEvent, latencyMs: 10_000, ttftMs: 2_000, streamed: true };
    const client = new GatewayUsageClient(new AdminClient({ fetchImpl: vi.fn<typeof fetch>() }));
    await act(async () => root.render(<EventsTable events={[tpsEvent]} hasMore={false} loadingMore={false} onLoadMore={() => {}}
      visibleColumns={['tps']} onVisibleColumnsChange={() => {}} onExport={() => {}} client={client} />));
    const cell = container.querySelectorAll('tbody tr td')[1];
    expect(cell.textContent).toBe('29.9 t/s');
    await act(async () => { cell.querySelector<HTMLButtonElement>('button')!.click(); });
    const popover = document.querySelector('[data-od-id="tps-breakdown"]');
    expect(popover?.textContent).toContain('平均输出速度');
    expect(popover?.textContent).toContain('首包时间');
    expect(popover?.textContent).toContain('2,000 ms');
    expect(popover?.textContent).not.toContain('生成时长');
    expect(popover?.textContent).toContain('10,000 ms');
    expect(popover?.textContent).toContain('不扣除首包时间');
  });

  it('uses the full latency as the throughput window for non-streaming requests', async () => {
    // 299 output tokens over 10s latency with no TTFT = 29.9 t/s.
    const tpsEvent: UsageEventViewModel = { ...richEvent, latencyMs: 10_000, ttftMs: undefined, streamed: false };
    const client = new GatewayUsageClient(new AdminClient({ fetchImpl: vi.fn<typeof fetch>() }));
    await act(async () => root.render(<EventsTable events={[tpsEvent]} hasMore={false} loadingMore={false} onLoadMore={() => {}}
      visibleColumns={['tps']} onVisibleColumnsChange={() => {}} onExport={() => {}} client={client} />));
    expect(container.querySelectorAll('tbody tr td')[1].textContent).toBe('29.9 t/s');
  });

  it('shows a dash for throughput when output usage is unreported or missing', async () => {
    const client = new GatewayUsageClient(new AdminClient({ fetchImpl: vi.fn<typeof fetch>() }));
    await act(async () => root.render(<EventsTable events={[fixtureEvents[1]]} hasMore={false} loadingMore={false} onLoadMore={() => {}}
      visibleColumns={['tps']} onVisibleColumnsChange={() => {}} onExport={() => {}} client={client} />));
    expect(container.querySelectorAll('tbody tr td')[1].textContent).toBe('—');
  });

  it('lists every token component with the usage source in the breakdowns', async () => {
    await act(async () => root.render(<><TokenBreakdown event={richEvent} /><CacheBreakdown event={richEvent} /><TpsBreakdown event={{ ...richEvent, ttftMs: 280, streamed: true }} /></>));
    const token = container.querySelector('[data-od-id="token-breakdown"]')!;
    for (const label of ['输入', '输出', '推理', '缓存读取', '缓存创建', '总计', '用量来源']) {
      expect(token.textContent).toContain(label);
    }
    expect(token.textContent).toContain('299');
    expect(token.textContent).toContain('143');
    expect(token.textContent).toContain('上游流式');
    const cache = container.querySelector('[data-od-id="cache-breakdown"]')!;
    expect(cache.textContent).toContain('命中率');
    expect(cache.textContent).toContain('96.4%');
    // 299 output tokens over 1280ms total latency = 234 t/s (rounded).
    const tps = container.querySelector('[data-od-id="tps-breakdown"]')!;
    for (const label of ['输出', '延迟', '首包时间', 'TPS']) {
      expect(tps.textContent).toContain(label);
    }
    expect(tps.textContent).toContain('1,280 ms');
    expect(tps.textContent).toContain('280 ms');
    expect(tps.textContent).not.toContain('生成时长');
    expect(tps.textContent).toContain('234 t/s');
  });
});

describe('virtual event table', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const base = adaptUsageEventPage(gatewayUsageEventsFixture).events[0];
  const events = Array.from({ length: 1000 }, (_, index) => ({ ...base, id: `event-${index}`, requestId: `request-${index}` }));
  beforeEach(async () => {
    await setTestLanguage('zh');
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    // happy-dom has no layout. Feed dimensions to the real virtualizer, rather
    // than replacing its range, key or measurement behavior with a test double.
    vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockImplementation(function (this: HTMLElement) {
      return this.tagName === 'TR' ? 74 : this.getAttribute('role') === 'region' ? 420 : 0;
    });
    vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockReturnValue(390);
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); vi.restoreAllMocks(); });

  it('highlights only added identities and expires highlights before virtual rows remount', async () => {
    const client = new GatewayUsageClient(new AdminClient({ fetchImpl: vi.fn<typeof fetch>() }));
    const render = (items: UsageEventViewModel[]) => root.render(<EventsTable events={items} hasMore={false}
      loadingMore={false} onLoadMore={() => {}} visibleColumns={['status']} onVisibleColumnsChange={() => {}} onExport={() => {}} client={client} />);
    await act(async () => render(events));
    expect(container.querySelector('[data-new="true"]')).toBeNull();
    vi.useFakeTimers();
    try {
      const updated = [{ ...base, id: 'new-request', requestId: 'new-request' }, ...events];
      act(() => render(updated));
      expect(container.querySelectorAll('[data-new="true"]')).toHaveLength(1);
      expect(container.querySelector('[data-new="true"] button')?.getAttribute('aria-label')).toContain('new-request');
      expect(container.querySelector('tr[data-index="1"]')?.getAttribute('data-new')).toBe('false');
      act(() => vi.advanceTimersByTime(1500));
      expect(container.querySelector('[data-new="true"]')).toBeNull();
      const region = container.querySelector<HTMLElement>('[role="region"]')!;
      act(() => { region.scrollTop = 24000; region.dispatchEvent(new Event('scroll')); });
      act(() => { region.scrollTop = 0; region.dispatchEvent(new Event('scroll')); });
      expect(container.querySelector('[data-new="true"]')).toBeNull();
    } finally { vi.useRealTimers(); }
  });

  it('measures rows, keeps a bounded DOM, and retains request identity after refresh', async () => {
    const client = new GatewayUsageClient(new AdminClient({ fetchImpl: vi.fn<typeof fetch>() }));
    const render = (items = events, columns: EventColumn[] = [...EVENT_COLUMNS]) => root.render(<EventsTable events={items} hasMore={false}
      loadingMore={false} onLoadMore={() => {}} visibleColumns={columns} onVisibleColumnsChange={() => {}} onExport={() => {}} client={client} />);
    await act(async () => render());
    const region = container.querySelector<HTMLElement>('[role="region"]')!;
    expect(region.tabIndex).toBe(0);
    expect(region.getAttribute('aria-label')).toBe('请求事件');
    expect(container.querySelector('table')?.getAttribute('aria-rowcount')).toBe('1001');
    expect(container.querySelectorAll('th[scope="col"]')).toHaveLength(EVENT_COLUMNS.length + 1);
    const row = container.querySelector<HTMLTableRowElement>('tbody tr')!;
    expect(container.querySelectorAll('tbody tr').length).toBeLessThan(40);
    expect(container.querySelector<HTMLTableRowElement>('tr[data-index="1"]')?.style.transform).toBe('translateY(74px)');
    await act(async () => render([events[1], events[0], ...events.slice(2)], ['status', 'usageSource']));
    expect(container.querySelector('tr[data-index="1"]')).toBe(row);
    expect(row.querySelector('button')?.getAttribute('aria-label')).toBe('查看请求 request-0 详情');
    expect(row.querySelectorAll('td')).toHaveLength(3);
    act(() => { region.scrollTop = 24000; region.dispatchEvent(new Event('scroll')); });
    expect(container.querySelectorAll('tbody tr').length).toBeLessThan(40);
    expect(Number(container.querySelector('tbody tr')?.getAttribute('aria-rowindex'))).toBeGreaterThan(100);
  });

  it('opens the selected request once and exports without opening details', async () => {
    const fetchImpl = vi.fn<typeof fetch>(async () => new Response(JSON.stringify({ items: [] })));
    const client = new GatewayUsageClient(new AdminClient({ fetchImpl }));
    const onExport = vi.fn();
    await act(async () => root.render(<EventsTable events={events} hasMore={false} loadingMore={false} onLoadMore={() => {}}
      visibleColumns={['model', 'status']} onVisibleColumnsChange={() => {}} onExport={onExport} client={client} />));
    const view = container.querySelector<HTMLButtonElement>('tbody button')!;
    await act(async () => { view.focus(); view.click(); });
    expect(fetchImpl).toHaveBeenCalledOnce();
    expect(String(fetchImpl.mock.calls[0][0])).toContain('/events/request-0');
    expect(container.querySelector('[role="dialog"]')?.textContent).toContain('request-0');
    await act(async () => root.render(<EventsTable events={[events[1], events[0], ...events.slice(2)]} hasMore={false} loadingMore={false} onLoadMore={() => {}}
      visibleColumns={['model', 'status']} onVisibleColumnsChange={() => {}} onExport={onExport} client={client} />));
    expect(container.querySelector('[role="dialog"]')?.textContent).toContain('request-0');
    expect(fetchImpl).toHaveBeenCalledOnce();
    act(() => container.querySelector('[role="dialog"] button')?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 30)); });
    expect(container.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(view);
    act(() => [...container.querySelectorAll<HTMLButtonElement>('button')].find(b => b.textContent === '导出 CSV')!.click());
    expect(onExport).toHaveBeenCalledWith('csv');
    expect(fetchImpl).toHaveBeenCalledOnce();
  });

  it('closes a detail whose row disappears and does not reopen it when rows return', async () => {
    const fetchImpl = vi.fn<typeof fetch>(async () => new Response(JSON.stringify({ items: [] })));
    const client = new GatewayUsageClient(new AdminClient({ fetchImpl }));
    const render = (items: typeof events) => root.render(<EventsTable events={items} hasMore={false}
      loadingMore={false} onLoadMore={() => {}} visibleColumns={['time']} onVisibleColumnsChange={() => {}}
      onExport={() => {}} client={client} />);
    await act(async () => render(events));
    const view = container.querySelector<HTMLButtonElement>('tbody button')!;
    await act(async () => { view.focus(); view.click(); });
    expect(container.querySelector('[role="dialog"]')?.textContent).toContain('request-0');

    await act(async () => render([]));
    expect(container.querySelector('[role="dialog"]')).toBeNull();
    const emptyFocus = container.querySelector<HTMLElement>('[data-od-id="events-empty-focus"]')!;
    expect(document.activeElement).toBe(emptyFocus);

    await act(async () => render(events));
    expect(container.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(container.querySelector('[role="region"]'));
    expect(fetchImpl).toHaveBeenCalledOnce();
  });

  it('loads the next page near the end and keeps existing rows while loading', async () => {
    const onLoadMore = vi.fn();
    const client = new GatewayUsageClient(new AdminClient({ fetchImpl: vi.fn<typeof fetch>() }));
    const render = (loadingMore: boolean, count: number) => root.render(<EventsTable events={events.slice(0, count)} hasMore loadingMore={loadingMore}
      onLoadMore={onLoadMore} visibleColumns={['time']} onVisibleColumnsChange={() => {}} onExport={() => {}} client={client} />);
    await act(async () => render(false, 30));
    expect(onLoadMore).not.toHaveBeenCalled();
    const region = container.querySelector<HTMLElement>('[role="region"]')!;
    act(() => { region.scrollTop = 2000; region.dispatchEvent(new Event('scroll')); });
    expect(onLoadMore).toHaveBeenCalledOnce();
    const row = container.querySelector('tr[data-index="29"]');
    await act(async () => render(true, 30));
    expect(container.querySelector('tr[data-index="29"]')).toBe(row);
    expect(container.querySelector('[role="status"]')?.textContent).toContain('加载更多事件');
    act(() => { region.scrollTop = 2010; region.dispatchEvent(new Event('scroll')); });
    expect(onLoadMore).toHaveBeenCalledOnce();
    await act(async () => render(false, 1000));
    expect(container.querySelector('tr[data-index="29"]')).toBe(row);
    expect(onLoadMore).toHaveBeenCalledOnce();
  });

  it('holds the failed page in place and retries it explicitly without restarting exports', async () => {
    const onLoadMore = vi.fn();
    const onRetryLoadMore = vi.fn();
    const onRetryExport = vi.fn();
    const onExport = vi.fn();
    const client = new GatewayUsageClient(new AdminClient({ fetchImpl: vi.fn<typeof fetch>() }));
    await act(async () => root.render(<EventsTable events={events.slice(0, 30)} hasMore loadingMore={false}
      loadMoreError="加载更多事件失败" onLoadMore={onLoadMore} onRetryLoadMore={onRetryLoadMore}
      visibleColumns={['time']} onVisibleColumnsChange={() => {}} onExport={onExport}
      exportError="用量导出失败" onRetryExport={onRetryExport} client={client} />));
    const region = container.querySelector<HTMLElement>('[role="region"]')!;
    act(() => { region.scrollTop = 2000; region.dispatchEvent(new Event('scroll')); });
    expect(onLoadMore).not.toHaveBeenCalled();
    const scrollTop = region.scrollTop;
    const alerts = [...container.querySelectorAll<HTMLElement>('[role="alert"]')];
    expect(alerts.map((alert) => alert.textContent)).toEqual(expect.arrayContaining([
      expect.stringContaining('加载更多事件失败'),
      expect.stringContaining('用量导出失败'),
    ]));
    const retryButtons = [...container.querySelectorAll<HTMLButtonElement>('button')].filter((button) => button.textContent === '重试');
    expect(retryButtons).toHaveLength(2);
    act(() => retryButtons[0].click());
    act(() => retryButtons[1].click());
    expect(onRetryExport).toHaveBeenCalledOnce();
    expect(onRetryLoadMore).toHaveBeenCalledOnce();
    expect(onExport).not.toHaveBeenCalled();
    expect(region.scrollTop).toBe(scrollTop);
  });

  it('uses named focusable metric explanations in table headers', async () => {
    const client = new GatewayUsageClient(new AdminClient({ fetchImpl: vi.fn<typeof fetch>() }));
    await act(async () => root.render(<EventsTable events={[base]} hasMore={false} loadingMore={false} onLoadMore={() => {}}
      visibleColumns={['tokens', 'tps', 'cache']} onVisibleColumnsChange={() => {}} onExport={() => {}} client={client} />));
    const explanation = container.querySelector<HTMLButtonElement>('button[aria-label="最终逻辑请求口径,不因回退重复累计"]')!;
    expect(explanation).not.toBeNull();
    expect(explanation.type).toBe('button');
    await act(async () => explanation.focus());
    expect(document.activeElement).toBe(explanation);
  });

});
