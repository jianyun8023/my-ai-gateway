// @vitest-environment happy-dom
import App from '@/App';
import { setTestLanguage } from '@/test/setup';
import { act } from 'react';
import { createRoot } from '@/test/render';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

describe('GatewayUsagePage empty state', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeAll(async () => {
    await import('@/pages/GatewayUsagePage');
  });

  beforeEach(async () => {
    await setTestLanguage('zh');
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    window.location.hash = '#overview';
    localStorage.clear();
    sessionStorage.clear();
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes('/summary')) return new Response(JSON.stringify({ logical_requests: { total: 0, successes: 0 }, tokens: {} }), { status: 200 });
      return new Response(JSON.stringify({ items: [], has_more: false }), { status: 200 });
    }));
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  it('renders a token-first empty state without pricing or CPA product surfaces', async () => {
    await act(async () => {
      root.render(<App />);
      await new Promise((resolve) => setTimeout(resolve, 100));
    });
    expect(container.textContent).toContain('当前范围暂无用量');
    expect(container.textContent).toContain('调整时间范围或筛选条件后重试');
    expect(container.textContent).not.toMatch(/Ranking|Auth Files|充值|配额/);
  });

  it('shows an initial event failure without also claiming the query succeeded empty', async () => {
    window.location.hash = '#events';
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      error: { code: 'events_unavailable', message: 'Synthetic event failure' },
    }), { status: 503, headers: { 'Content-Type': 'application/json' } })));
    await act(async () => {
      root.render(<App />);
      await new Promise((resolve) => setTimeout(resolve, 100));
    });
    expect(container.querySelector('[role="alert"]')?.textContent).toContain('服务暂时不可用');
    expect(container.textContent).not.toContain('当前范围没有请求事件');
  });

  it('treats an invalid custom range as validation and offers no stale-query retry', async () => {
    await act(async () => {
      root.render(<App />);
      await new Promise((resolve) => setTimeout(resolve, 100));
    });
    const requestCount = vi.mocked(fetch).mock.calls.length;
    act(() => [...container.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent === '自定义')!.click());
    const input = container.querySelector<HTMLInputElement>('input[type="datetime-local"]')!;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '');
      input.dispatchEvent(new Event('input', { bubbles: true }));
      [...container.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent === '应用筛选')!.click();
    });
    const alert = container.querySelector<HTMLElement>('[role="alert"]')!;
    expect(alert.textContent).toContain('开始时间必须早于结束时间');
    expect(alert.querySelector('button')).toBeNull();
    expect(vi.mocked(fetch)).toHaveBeenCalledTimes(requestCount);
  });
});
