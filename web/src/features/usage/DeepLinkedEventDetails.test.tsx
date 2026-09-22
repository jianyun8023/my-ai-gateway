// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from '@/test/render';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AdminClient } from '@/admin-api/client';
import { GatewayUsageClient } from '@/gateway-usage/client';
import { gatewayUsageEventsFixture } from '@/test/fixtures/usage';
import { setTestLanguage } from '@/test/setup';
import { DeepLinkedEventDetails } from './DeepLinkedEventDetails';

const requestId = 'historical/request 1';
const event = { ...gatewayUsageEventsFixture.items[0], request_id: requestId };
const response = () => new Response(JSON.stringify({ version: 'v1', data: event, attempts: event.attempts }));

describe('deep-linked request details', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const fetchImpl = vi.fn<typeof fetch>();
  const client = new GatewayUsageClient(new AdminClient({ fetchImpl }));
  const render = async (id = requestId, generation = 0, activeClient = client) => {
    await act(async () => root.render(<DeepLinkedEventDetails key={`${generation}:${id}`} requestId={id} client={activeClient} />));
  };

  beforeEach(async () => {
    vi.resetAllMocks();
    await setTestLanguage('zh');
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
    window.location.hash = '#events?request_id=historical%2Frequest%201';
  });
  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  it('opens a historical event using its exact ID, with real fields and attempts', async () => {
    fetchImpl.mockResolvedValue(response());
    await render();
    const dialog = document.querySelector('[role="dialog"]')!;
    expect(dialog.textContent).toContain(requestId);
    expect(dialog.textContent).toContain('reasoning-large');
    expect(dialog.textContent).toContain('source-tokyo');
    expect(dialog.textContent).toContain('source-singapore');
    expect(fetchImpl).toHaveBeenCalledTimes(1);
    expect(String(fetchImpl.mock.calls[0][0])).toBe('/admin/usage/events/historical%2Frequest%201');
    await act(async () => [...dialog.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === '关闭')!.click());
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 250)); });
    expect(window.location.hash).toBe('#events');
  });

  it('shows a failed lookup with retry and close, without inventing event fields', async () => {
    fetchImpl.mockResolvedValueOnce(new Response('{"error":{"code":"event_not_found"}}', { status: 404 })).mockResolvedValueOnce(response());
    await render();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(container.textContent).toContain(`无法加载请求 ${requestId}`);
    expect(container.textContent).not.toContain('reasoning-large');
    await act(async () => [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === '重试')!.click());
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain('reasoning-large');
    expect(fetchImpl).toHaveBeenCalledTimes(2);
  });

  it('aborts the old identity lookup when the Admin key changes, even for the same request ID', async () => {
    let resolveOld!: (value: Response) => void;
    let adminKey = 'old-key';
    const authClient = new GatewayUsageClient(new AdminClient({ fetchImpl, getAdminKey: () => adminKey }));
    fetchImpl.mockImplementationOnce(() => new Promise(resolve => { resolveOld = resolve; }));
    await render(requestId, 0, authClient);
    const oldSignal = fetchImpl.mock.calls[0][1]!.signal!;
    fetchImpl.mockResolvedValueOnce(new Response('{"error":{"code":"event_not_found"}}', { status: 404 }));
    adminKey = 'new-key';
    await render(requestId, 1, authClient);
    expect(oldSignal.aborted).toBe(true);
    expect(new Headers(fetchImpl.mock.calls[0][1]!.headers).get('Authorization')).toBe('Bearer old-key');
    expect(new Headers(fetchImpl.mock.calls[1][1]!.headers).get('Authorization')).toBe('Bearer new-key');
    await act(async () => resolveOld(response()));
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(container.textContent).toContain(requestId);
    expect(container.textContent).not.toContain('reasoning-large');
  });

  it('does not publish a late result after the route ID changes', async () => {
    let resolveOld!: (value: Response) => void;
    fetchImpl.mockImplementationOnce(() => new Promise(resolve => { resolveOld = resolve; }));
    await render();
    const oldSignal = fetchImpl.mock.calls[0][1]!.signal!;
    fetchImpl.mockResolvedValueOnce(new Response('{"error":{"code":"event_not_found"}}', { status: 404 }));
    await render('new-request');
    expect(oldSignal.aborted).toBe(true);
    await act(async () => resolveOld(response()));
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(container.textContent).toContain('new-request');
  });
});
