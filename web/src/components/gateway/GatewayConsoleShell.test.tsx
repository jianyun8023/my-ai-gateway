// @vitest-environment happy-dom
import { act } from 'react';
import { useThemeStore } from '@/stores/useThemeStore';
import { createRoot } from '@/test/render';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setTestLanguage } from '@/test/setup';
import { IconDashboardGrid } from '@/components/ui/icons';
import { GatewayConsoleShell, GATEWAY_ADMIN_KEY_STORAGE_KEY } from './GatewayConsoleShell';
import packageJson from '../../../package.json';

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

  const openConnection = async () => {
    const trigger = container.querySelector<HTMLButtonElement>('button[aria-haspopup="dialog"]')!;
    await act(async () => { trigger.focus(); trigger.click(); });
    return trigger;
  };

  it('keeps the applied secret out of text, URL, localStorage, and logs', async () => {
    let readAdminKey = () => '';
    let clearAdminKey = () => {};
    let refreshRevision = -1;
    let authGeneration = -1;
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
            clearAdminKey = context.clearAdminKey;
            refreshRevision = context.refreshRevision;
            authGeneration = context.authGeneration;
            return <div>content</div>;
          }}
        </GatewayConsoleShell>,
      );
    });

    expect(container.textContent).toContain(`v${packageJson.version}`);
    expect(container.querySelector('input[aria-label="Admin Key"]')).toBeNull();
    await openConnection();
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
    await act(async () => apply?.click());

    expect(readAdminKey()).toBe('top-secret-value');
    expect(refreshRevision).toBe(1);
    expect(authGeneration).toBe(1);
    expect(sessionStorage.getItem(GATEWAY_ADMIN_KEY_STORAGE_KEY)).toBe('top-secret-value');
    await openConnection();
    expect(container.querySelector<HTMLInputElement>('input[aria-label="Admin Key"]')!.value).toBe('');
    expect(localStorage.getItem(GATEWAY_ADMIN_KEY_STORAGE_KEY)).toBeNull();
    expect(container.textContent).not.toContain('top-secret-value');
    expect(window.location.href).not.toContain('top-secret-value');
    expect(consoleError).not.toHaveBeenCalled();

    act(() => clearAdminKey());
    expect(readAdminKey()).toBe('');
    expect(refreshRevision).toBe(2);
    expect(authGeneration).toBe(2);
    expect(sessionStorage.getItem(GATEWAY_ADMIN_KEY_STORAGE_KEY)).toBeNull();
  });

  it('previews theme style and mode while blocking repeated refresh during work', async () => {
    vi.stubGlobal('matchMedia', (query: string) => ({ matches: query.includes('prefers-color-scheme: dark'), media: query,
      addEventListener: () => {}, removeEventListener: () => {} }));
    useThemeStore.getState().setStyle('utility');
    useThemeStore.getState().setMode('auto');
    let context: Parameters<Parameters<typeof GatewayConsoleShell>[0]['children']>[0];
    await act(async () => root.render(<GatewayConsoleShell activePage="overview" title="总览" navigationSections={[]}
      navigationItems={[]} onNavigate={() => {}} refreshable>{(value) => { context = value; return <div>content</div>; }}</GatewayConsoleShell>));
    const appearance = container.querySelector<HTMLButtonElement>('button[aria-label^="外观设置"]')!;
    expect(appearance).not.toBeNull();
    await act(async () => appearance.click());
    const nebula = document.querySelector<HTMLButtonElement>('button[data-theme-option="nebula"]')!;
    const light = document.querySelector<HTMLButtonElement>('button[data-mode-option="light"]')!;
    await act(async () => { nebula.click(); light.click(); });
    expect(useThemeStore.getState().style).toBe('nebula');
    expect(useThemeStore.getState().mode).toBe('light');
    expect(useThemeStore.getState().resolvedColorScheme).toBe('light');
    expect(document.documentElement.getAttribute('data-theme-style')).toBe('nebula');
    expect(document.documentElement.getAttribute('data-color-scheme')).toBe('light');
    const refresh = container.querySelector<HTMLButtonElement>('button[aria-label="刷新"]')!;
    expect(context!.authGeneration).toBe(0);
    act(() => refresh.click());
    expect(context!.refreshRevision).toBe(1);
    expect(context!.authGeneration).toBe(0);
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
    const connectionTrigger = await openConnection();
    const input = container.querySelector<HTMLInputElement>('input[aria-label="Admin Key"]')!;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'draft-demo-key');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 30)); });
    expect(document.activeElement).toBe(connectionTrigger);
    expect(context!.getAdminKey()).toBe('');
    expect(context!.refreshRevision).toBe(0);
    await openConnection();
    expect(container.querySelector<HTMLInputElement>('input[aria-label="Admin Key"]')!.value).toBe('draft-demo-key');
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
    await openConnection();
    const restored = container.querySelector<HTMLInputElement>('input[aria-label="Admin Key"]')!;
    expect(restored.value).toBe('draft-demo-key');
    await act(async () => restored.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })));
    expect(context!.getAdminKey()).toBe('draft-demo-key');
    expect(context!.refreshRevision).toBe(1);
    expect(context!.authGeneration).toBe(1);
    await openConnection();
    expect(container.querySelector<HTMLInputElement>('input[aria-label="Admin Key"]')!.value).toBe('');
    expect(container.textContent).not.toContain('draft-demo-key');
  });

});
