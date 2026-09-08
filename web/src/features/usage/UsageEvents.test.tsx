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
import { EventsTable } from './UsageEvents';

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
      visibleColumns={['logicalModel', 'status']} onVisibleColumnsChange={() => {}} onExport={onExport} client={client} />));
    const view = container.querySelector<HTMLButtonElement>('tbody button')!;
    await act(async () => { view.focus(); view.click(); });
    expect(fetchImpl).toHaveBeenCalledOnce();
    expect(String(fetchImpl.mock.calls[0][0])).toContain('/events/request-0');
    expect(container.querySelector('[role="dialog"]')?.textContent).toContain('request-0');
    await act(async () => root.render(<EventsTable events={[events[1], events[0], ...events.slice(2)]} hasMore={false} loadingMore={false} onLoadMore={() => {}}
      visibleColumns={['logicalModel', 'status']} onVisibleColumnsChange={() => {}} onExport={onExport} client={client} />));
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

});
