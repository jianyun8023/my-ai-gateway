// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { GatewayUsagePage } from './GatewayUsagePage';

describe('GatewayUsagePage mobile navigation', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    window.location.hash = '#overview';
    document.body.style.overflow = '';
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
    document.body.style.overflow = '';
    vi.unstubAllGlobals();
  });

  it('locks background scrolling and closes on Escape', async () => {
    await act(async () => {
      root.render(<GatewayUsagePage />);
      await new Promise((resolve) => setTimeout(resolve, 20));
    });

    const openButton = container.querySelector<HTMLButtonElement>('button[aria-label="打开导航"]');
    const sidebar = container.querySelector<HTMLElement>('[data-od-id="sidebar"]');
    expect(openButton).not.toBeNull();
    expect(sidebar?.dataset.open).toBe('false');

    act(() => openButton?.click());
    expect(sidebar?.dataset.open).toBe('true');
    expect(document.body.style.overflow).toBe('hidden');

    act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' })));
    expect(sidebar?.dataset.open).toBe('false');
    expect(document.body.style.overflow).toBe('');
  });

  it('closes the drawer after navigation', async () => {
    await act(async () => {
      root.render(<GatewayUsagePage />);
      await new Promise((resolve) => setTimeout(resolve, 20));
    });

    act(() => container.querySelector<HTMLButtonElement>('button[aria-label="打开导航"]')?.click());
    const analysisButton = Array.from(container.querySelectorAll<HTMLButtonElement>('nav[aria-label="主导航"] button'))
      .find((button) => button.textContent?.includes('Analysis'));
    act(() => analysisButton?.click());

    expect(container.querySelector<HTMLElement>('[data-od-id="sidebar"]')?.dataset.open).toBe('false');
    expect(document.body.style.overflow).toBe('');
  });
});
