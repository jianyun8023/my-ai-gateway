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
