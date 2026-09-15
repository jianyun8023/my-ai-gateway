// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from '@/test/render';
import { setTestLanguage } from '@/test/setup';
import { GATEWAY_ADMIN_KEY_STORAGE_KEY } from '@/components/gateway/GatewayConsoleShell';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';

const event = {
  event_id: 'system:identity-a', occurred_at: '2026-09-15T00:00:00Z',
  category: 'database', event_type: 'database.connection_failed', level: 'error',
  subject_type: 'database', subject_id: 'postgresql', correlation_id: null,
  message: 'Identity A event', details: { component: 'identity-a-detail' }, source: 'system_events',
};
const events = (data = [event]) => ({
  version: 'v1', timezone: 'UTC', fact_source: 'postgresql_unified_read_model',
  range: { from: null, to: null, boundary: '[from,to)' }, data,
  page: { limit: 100, has_more: true, next_cursor: 'identity-a-cursor' },
});
const json = (data: unknown, status = 200) => new Response(JSON.stringify(data), { status });
const unauthorized = () => json({ error: { code: 'unauthorized', message: 'Unauthorized' } }, 401);

async function waitFor(condition: () => boolean) {
  const deadline = Date.now() + 1000;
  while (!condition() && Date.now() < deadline) {
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)); });
  }
  expect(condition()).toBe(true);
}

describe('console connection identity', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeAll(async () => {
    await setTestLanguage('zh');
    await import('./pages/GatewayManagementPage');
  });
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    sessionStorage.clear();
    sessionStorage.setItem(GATEWAY_ADMIN_KEY_STORAGE_KEY, 'identity-a');
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  const mount = async (hash: string) => {
    window.location.hash = hash;
    await act(async () => root.render(<App />));
    await waitFor(() => container.querySelector('table') !== null);
  };
  const click = async (text: string) => {
    const button = [...container.querySelectorAll('button')].find((item) => item.textContent === text)!;
    expect(button).toBeDefined();
    await act(async () => button.click());
  };
  const openDetails = async () => {
    await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="查看事件 system:identity-a"]')!.click());
    expect(container.textContent).toContain('identity-a-detail');
  };

  it('discards old rows, details and pending cursor results when a new key fails', async () => {
    let finishCursor!: (value: Response) => void;
    let cursorSignal!: AbortSignal;
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      if (new Headers(init?.headers).get('Authorization') !== 'Bearer identity-a') return Promise.resolve(unauthorized());
      if (String(input).includes('cursor=')) {
        cursorSignal = init!.signal!;
        return new Promise<Response>((resolve) => { finishCursor = resolve; });
      }
      return Promise.resolve(json(events()));
    });
    vi.stubGlobal('fetch', fetchMock);
    await mount('#runtime-events');
    await click('加载更多');
    await openDetails();
    await act(async () => container.querySelector<HTMLButtonElement>('button[aria-haspopup="dialog"]')!.click());
    const input = container.querySelector<HTMLInputElement>('input[aria-label="Admin Key"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'identity-b');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await click('应用');
    await waitFor(() => container.textContent?.includes('unauthorized') ?? false);
    expect(cursorSignal.aborted).toBe(true);
    expect(container.textContent).not.toContain('Identity A event');
    expect(container.textContent).not.toContain('identity-a-detail');
    await act(async () => finishCursor(json(events([{ ...event, event_id: 'system:late', message: 'Late A event' }]))));
    expect(container.textContent).not.toContain('Late A event');
  });

  it('clears management data when the session key is removed', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      if (!new Headers(init?.headers).has('Authorization')) return unauthorized();
      if (String(input) === '/admin/keys') return json({ data: [{
        id: 'vk-a', name: 'Identity A virtual key', key_prefix: 'test', allowed_models: [],
        created_at: '2026-09-15T00:00:00Z', enabled: true,
      }] });
      return json({ data: [], snapshot_revision: 1, snapshot_generated_at: '2026-09-15T00:00:00Z' });
    }));
    await mount('#settings');
    expect(container.textContent).toContain('Identity A virtual key');
    await click('清除会话密钥');
    await waitFor(() => container.querySelector('table') === null);
    expect(sessionStorage.getItem(GATEWAY_ADMIN_KEY_STORAGE_KEY)).toBeNull();
    expect(container.textContent).not.toContain('Identity A virtual key');
  });

  it('keeps current rows and an open detail during a same-identity refresh failure', async () => {
    let requests = 0;
    vi.stubGlobal('fetch', vi.fn(async () => ++requests === 1 ? json(events()) : json({ error: { code: 'temporary_failure' } }, 503)));
    await mount('#runtime-events');
    await openDetails();
    await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="刷新"]')!.click());
    await waitFor(() => container.textContent?.includes('temporary_failure') ?? false);
    expect(container.textContent).toContain('Identity A event');
    expect(container.textContent).toContain('identity-a-detail');
  });
});
