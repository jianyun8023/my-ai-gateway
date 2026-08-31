// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';

describe('App console routing', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeAll(async () => {
    await Promise.all([
      import('./pages/GatewayManagementPage'),
      import('./pages/GatewayUsagePage'),
    ]);
  });

  beforeEach(() => {
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

  it('deep-links Management separately and switches back to exactly three Usage pages', async () => {
    window.location.hash = '#management/capabilities';
    await act(async () => {
      root.render(<App />);
      await new Promise((resolve) => setTimeout(resolve, 500));
    });

    expect(container.querySelector('nav[aria-label="管理导航"]')).not.toBeNull();
    await vi.waitFor(() => expect(container.querySelector('[data-od-id="page-capabilities"]')).not.toBeNull(), { timeout: 3000 });
    expect(container.textContent).toContain('runtime snapshot');
    expect(container.textContent).not.toMatch(/Ranking|Auth Files|充值|配额/);

    const usageButton = Array.from(container.querySelectorAll<HTMLButtonElement>('[role="group"][aria-label="工作空间"] button'))
      .find((button) => button.textContent?.includes('Usage'));
    act(() => usageButton?.click());
    await act(async () => new Promise((resolve) => setTimeout(resolve, 500)));

    const usageButtons = container.querySelectorAll('nav[aria-label="主导航"] button');
    expect(usageButtons).toHaveLength(3);
    expect(Array.from(usageButtons).map((button) => button.textContent)).toEqual(expect.arrayContaining([
      expect.stringContaining('Overview'),
      expect.stringContaining('Analysis'),
      expect.stringContaining('Request Events'),
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
      window.location.hash = '#management/model-discovery';
      window.dispatchEvent(new HashChangeEvent('hashchange'));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    await vi.waitFor(() => expect(container.querySelector('nav[aria-label="管理导航"]')).not.toBeNull(), { timeout: 3000 });
    expect(container.textContent).toContain('Model Discovery');
  });
});
