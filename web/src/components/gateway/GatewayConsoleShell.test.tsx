// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IconFilterAll } from '@/components/ui/icons';
import { GatewayConsoleShell, GATEWAY_ADMIN_KEY_STORAGE_KEY } from './GatewayConsoleShell';

describe('GatewayConsoleShell Admin key boundary', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
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
  });

  it('keeps the applied secret out of text, URL, localStorage, and logs', () => {
    let readAdminKey = () => '';
    let refreshRevision = -1;
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    act(() => {
      root.render(
        <GatewayConsoleShell
          space="usage"
          navigationLabel="主导航"
          navigationSection="监控"
          navigationItems={[{ id: 'overview', label: 'Overview', shortLabel: '总览', icon: <IconFilterAll /> }]}
          activeItem="overview"
          onNavigate={() => {}}
          onSpaceChange={() => {}}
          title="Overview"
          shortTitle="总览"
          eyebrow="Usage"
          description="Token"
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
    expect(input.placeholder).toBe('输入 GATEWAY_ADMIN_KEY');
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
    expect(localStorage.getItem(GATEWAY_ADMIN_KEY_STORAGE_KEY)).toBeNull();
    expect(container.textContent).not.toContain('top-secret-value');
    expect(window.location.href).not.toContain('top-secret-value');
    expect(consoleError).not.toHaveBeenCalled();
  });
});
