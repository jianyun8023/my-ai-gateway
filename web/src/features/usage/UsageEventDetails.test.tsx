// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AdminClient } from '@/admin-api/client';
import { GatewayUsageClient } from '@/gateway-usage/client';
import { adaptUsageEventPage } from '@/gateway-usage/adapter';
import { gatewayUsageEventsFixture } from '@/test/fixtures/usage';
import { setTestLanguage } from '@/test/setup';
import { EventDetails } from './UsageEventDetails';

const event = { ...adaptUsageEventPage(gatewayUsageEventsFixture).events[0], attempts: [] };

describe('usage event details', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const fetchImpl = vi.fn<typeof fetch>();
  const onClose = vi.fn();
  const render = async () => {
    const client = new GatewayUsageClient(new AdminClient({ fetchImpl }));
    await act(async () => root.render(<EventDetails event={event} client={client} onClose={onClose} />));
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
    expect(dialog.textContent).toContain('source-b');
    expect(dialog.textContent).not.toContain('暂时不可用');
    act(() => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
    expect(onClose).toHaveBeenCalledOnce();
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
