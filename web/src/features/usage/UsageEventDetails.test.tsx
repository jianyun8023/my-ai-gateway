// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from '@/test/render';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AdminClient } from '@/admin-api/client';
import type { UsageEventViewModel } from "@/gateway-usage";
import { GatewayUsageClient } from '@/gateway-usage/client';
import { adaptUsageEventPage } from '@/gateway-usage/adapter';
import { gatewayUsageEventsFixture } from '@/test/fixtures/usage';
import { setTestLanguage } from '@/test/setup';
import { EventDetails } from './UsageEventDetails';

const event: UsageEventViewModel = { ...adaptUsageEventPage(gatewayUsageEventsFixture).events[0], attempts: [] };

describe('usage event details', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const fetchImpl = vi.fn<typeof fetch>();
  const onClose = vi.fn();
  const render = async (selectedEvent = event) => {
    const client = new GatewayUsageClient(new AdminClient({ fetchImpl }));
    await act(async () => root.render(<EventDetails event={selectedEvent} client={client} onClose={onClose} />));
  };
  beforeEach(async () => {
    vi.resetAllMocks();
    await setTestLanguage('zh');
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  it('shows a failed detail request with retry instead of claiming attempts are empty', async () => {
    fetchImpl.mockResolvedValueOnce(new Response('{"error":{"code":"unavailable"}}', { status: 503 }));
    await render();
    const dialog = document.querySelector('[role="dialog"]')!;
    expect(dialog.textContent).toContain('暂时不可用');
    expect(dialog.textContent).not.toContain('暂无上游尝试');
    fetchImpl.mockResolvedValueOnce(new Response(JSON.stringify({ attempts: [{ attempt_no: 0, provider_id: 'provider-b', source_id: 'source-b', account_id: 'account-b', upstream_model_id: 'model-b', status_code: 200, success: true, latency_ms: 20 }] })));
    await act(async () => [...dialog.querySelectorAll('button')].find(button => button.textContent === '重试')!.click());
    expect(dialog.textContent).toContain('account-b');
    // A single attempt is compressed to one row; full attribution stays in the title.
    const attemptRow = dialog.querySelector('[title*="source-b"]');
    expect(attemptRow?.getAttribute('title')).toContain('provider-b');
    expect(attemptRow?.getAttribute('title')).toContain('model-b');
    expect(dialog.textContent).not.toContain('暂时不可用');
    act(() => dialog.querySelector('button')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 250)); });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('keeps exact token totals, zero latency, protocol attribution and text attempt statuses', async () => {
    fetchImpl.mockResolvedValue(new Response(JSON.stringify({ attempts: gatewayUsageEventsFixture.items[0].attempts })));
    await render({ ...event, latencyMs: 0, tokens: { ...event.tokens, total: 1234567890123 } });
    const dialog = document.querySelector('[role="dialog"]')!;
    const values = Object.fromEntries([...dialog.querySelectorAll('dt')].map(term => [term.textContent, term.nextElementSibling?.textContent]));
    expect(values).toMatchObject({ '逻辑模型': event.logicalModel, '上游模型': event.upstreamModel, '来源 ID': event.sourceId });
    expect(values['协议']).toBe(`${event.protocolIn} → ${event.protocolUpstream}`);
    expect(dialog.textContent).toContain('1,234,567,890,123');
    expect(dialog.textContent).toContain('0 ms');
    expect(dialog.textContent).toContain('429 · 失败');
    expect(dialog.textContent).toContain('200 · 成功');
    expect(dialog.textContent).toContain('anthropic_messages');
    expect(dialog.textContent).toContain('source-singapore');
    expect(dialog.textContent).toContain('source-tokyo');
  });

  it('explains missing token usage instead of displaying unexplained accounting zeros', async () => {
    fetchImpl.mockResolvedValue(new Response(JSON.stringify({ attempts: [] })));
    await render({ ...event, ...adaptUsageEventPage(gatewayUsageEventsFixture).events[1] });
    const dialog = document.querySelector('[role="dialog"]')!;
    expect(dialog.textContent).toContain('记账零不代表已确认零用量');
    const values = Object.fromEntries([...dialog.querySelectorAll('dt')].map(term => [term.textContent, term.nextElementSibling?.textContent]));
    expect(values['总计']).toBe('—');
    expect(values['命中率']).toBe('—');
    expect(dialog.textContent).toContain('未获取');
  });

  it('cancels the detail request when the drawer is removed', async () => {
    fetchImpl.mockImplementation(() => new Promise(() => {}));
    await render();
    const signal = fetchImpl.mock.calls[0][1]!.signal!;
    act(() => root.render(null));
    expect(signal.aborted).toBe(true);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });
});
