// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { setTestLanguage } from '@/test/setup';
import App from './App';

describe('App console routing', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeAll(async () => {
    await setTestLanguage('zh');
    await Promise.all([
      import('./pages/GatewayManagementPage'),
      import('./pages/GatewayUsagePage'),
    ]);
  });

  beforeEach(async () => {
    await setTestLanguage('zh');
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    localStorage.clear();
    sessionStorage.clear();
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url === '/admin/capabilities') return new Response(JSON.stringify({
        version: 'v1',
        fact_source: 'runtime_snapshot',
        snapshot_revision: 1,
        snapshot_generated_at: '2026-08-31T00:00:00Z',
        data: [],
      }), { status: 200 });
      if (url.includes('/summary')) return new Response(JSON.stringify({ logical_requests: { total: 0, successes: 0 }, tokens: {} }), { status: 200 });
      if (url.startsWith('/admin/')) return new Response(JSON.stringify({ data: [] }), { status: 200 });
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

  it('flat sidebar shows all 8 navigation pages', async () => {
    window.location.hash = '#overview';
    await act(async () => {
      root.render(<App />);
      await new Promise((resolve) => setTimeout(resolve, 500));
    });

    const navButtons = container.querySelectorAll('nav[aria-label="主导航"] button');
    expect(navButtons).toHaveLength(8);
    expect(Array.from(navButtons).map((b) => b.textContent)).toEqual(expect.arrayContaining([
      expect.stringContaining('总览'),
      expect.stringContaining('用量分析'),
      expect.stringContaining('请求事件'),
      expect.stringContaining('来源管理'),
      expect.stringContaining('模型发现'),
      expect.stringContaining('模型与路由'),
      expect.stringContaining('能力矩阵'),
      expect.stringContaining('系统设置'),
    ]));
  });

  it('reacts to browser history hash changes and canonicalizes unknown routes', async () => {
    window.location.hash = '#ranking';
    await act(async () => {
      root.render(<App />);
      await new Promise((resolve) => setTimeout(resolve, 500));
    });
    expect(window.location.hash).toBe('#overview');

    await act(async () => {
      window.location.hash = '#sources';
      window.dispatchEvent(new HashChangeEvent('hashchange'));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(window.location.hash).toBe('#sources');
  });
});
