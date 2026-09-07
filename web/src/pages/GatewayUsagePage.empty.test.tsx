// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '@/App';
import { setTestLanguage } from '@/test/setup';

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
});
