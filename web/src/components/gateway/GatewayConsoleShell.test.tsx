// @vitest-environment happy-dom
import { act } from 'react';
import { useThemeStore } from '@/stores/useThemeStore';
import { createRoot } from '@/test/render';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setTestLanguage } from '@/test/setup';
import { IconDashboardGrid } from '@/components/ui/icons';
import { GatewayConsoleShell, GATEWAY_ADMIN_KEY_STORAGE_KEY } from './GatewayConsoleShell';

describe('GatewayConsoleShell Admin key boundary', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(async () => {
    await setTestLanguage('zh');
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    localStorage.clear();
    sessionStorage.clear();
    window.location.hash = '#overview';
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('keeps the applied secret out of text, URL, localStorage, and logs', () => {
    let readAdminKey = () => '';
    let refreshRevision = -1;
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    act(() => {
      root.render(
        <GatewayConsoleShell
          activePage="overview"
          navigationSections={[{ label: '监控', pages: ['overview'] }]}
          navigationItems={[{ id: 'overview', label: '总览', icon: <IconDashboardGrid /> }]}
          onNavigate={() => {}}
          title="总览"
          refreshable
        >
          {(context) => {
            readAdminKey = context.getAdminKey;
            refreshRevision = context.refreshRevision;
            return <div>content</div>;
          }}
        </GatewayConsoleShell>,
      );
    });

    const input = container.querySelector<HTMLInputElement>('input[aria-label="Admin Key"]')!;
    expect(input.required).toBe(true);
    expect(input.placeholder).toBe('GATEWAY_ADMIN_KEY');
    act(() => {
      const valueSetter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
      valueSetter?.call(input, 'top-secret-value');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    const apply = Array.from(container.querySelectorAll<HTMLButtonElement>('button'))
      .find((button) => button.textContent?.includes('应用'));
    act(() => apply?.click());

    expect(readAdminKey()).toBe('top-secret-value');
    expect(refreshRevision).toBe(1);
    expect(sessionStorage.getItem(GATEWAY_ADMIN_KEY_STORAGE_KEY)).toBe('top-secret-value');
    expect(input.value).toBe('');
    expect(localStorage.getItem(GATEWAY_ADMIN_KEY_STORAGE_KEY)).toBeNull();
    expect(container.textContent).not.toContain('top-secret-value');
    expect(window.location.href).not.toContain('top-secret-value');
    expect(consoleError).not.toHaveBeenCalled();
  });

  it('toggles the resolved system-dark theme and blocks repeated refresh while busy', async () => {
    vi.stubGlobal('matchMedia', (query: string) => ({ matches: query.includes('prefers-color-scheme: dark'), media: query,
      addEventListener: () => {}, removeEventListener: () => {} }));
    useThemeStore.getState().setTheme('auto');
    let context: Parameters<Parameters<typeof GatewayConsoleShell>[0]['children']>[0];
    await act(async () => root.render(<GatewayConsoleShell activePage="overview" title="总览" navigationSections={[]}
      navigationItems={[]} onNavigate={() => {}} refreshable>{(value) => { context = value; return <div>content</div>; }}</GatewayConsoleShell>));
    const toggle = container.querySelector<HTMLButtonElement>('button[aria-label="切换为浅色主题"]')!;
    expect(toggle).not.toBeNull();
    act(() => toggle.click());
    expect(useThemeStore.getState().resolvedTheme).toBe('light');
    expect(container.querySelector('button[aria-label="切换为深色主题"]')).not.toBeNull();
    const refresh = container.querySelector<HTMLButtonElement>('button[aria-label="刷新"]')!;
    act(() => refresh.click());
    expect(context!.refreshRevision).toBe(1);
    act(() => context!.setRefreshing(true));
    expect(refresh.disabled).toBe(true);
    act(() => refresh.click());
    expect(context!.refreshRevision).toBe(1);
    act(() => context!.setRefreshing(false));
    act(() => refresh.click());
    expect(context!.refreshRevision).toBe(2);
  });

  it('preserves an unapplied key draft across responsive layouts without applying it on navigation', async () => {
    let mobile = false;
    const listeners = new Map<(event: { matches: boolean }) => void, string>();
    vi.stubGlobal('matchMedia', (query: string) => ({ get matches() { return query.includes('max-width') && mobile; }, media: query,
      addEventListener: (_type: string, callback: (event: { matches: boolean }) => void) => listeners.set(callback, query),
      removeEventListener: (_type: string, callback: (event: { matches: boolean }) => void) => listeners.delete(callback) }));
    let context: Parameters<Parameters<typeof GatewayConsoleShell>[0]['children']>[0];
    await act(async () => root.render(<GatewayConsoleShell activePage="overview" title="总览"
      navigationSections={[{ label: '监控', pages: ['overview'] }]} navigationItems={[{ id: 'overview', label: '总览', icon: <IconDashboardGrid /> }]}
      onNavigate={() => {}}>{(value) => { context = value; return <div>content</div>; }}</GatewayConsoleShell>));
    const input = container.querySelector<HTMLInputElement>('input[aria-label="Admin Key"]')!;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'draft-demo-key');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await act(async () => { mobile = true; listeners.forEach((query, callback) => callback({ matches: query.includes('max-width') && mobile })); });
    expect(container.querySelector('input[aria-label="Admin Key"]')).toBeNull();
    const open = container.querySelector<HTMLButtonElement>('button[aria-label="打开导航"]')!;
    await act(async () => { open.focus(); open.click(); });
    const mobileInput = container.querySelector<HTMLInputElement>('[role="dialog"] input[aria-label="Admin Key"]')!;
    expect(mobileInput.value).toBe('draft-demo-key');
    expect(context!.getAdminKey()).toBe('');
    expect(sessionStorage.getItem(GATEWAY_ADMIN_KEY_STORAGE_KEY)).toBeNull();
    act(() => mobileInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 30)); });
    expect(document.activeElement).toBe(open);
    expect(context!.refreshRevision).toBe(0);
    await act(async () => { mobile = false; listeners.forEach((query, callback) => callback({ matches: query.includes('max-width') && mobile })); });
    const restored = container.querySelector<HTMLInputElement>('input[aria-label="Admin Key"]')!;
    expect(restored.value).toBe('draft-demo-key');
    act(() => restored.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })));
    expect(context!.getAdminKey()).toBe('draft-demo-key');
    expect(context!.refreshRevision).toBe(1);
    expect(restored.value).toBe('');
    expect(container.textContent).not.toContain('draft-demo-key');
  });

});
