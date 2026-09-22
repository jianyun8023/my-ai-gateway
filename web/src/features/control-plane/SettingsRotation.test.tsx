// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from '@/test/render';
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import { GatewayAdminResources, type CapabilityMatrixResponse, type VirtualKey } from '@/admin-api';
import type { AdminTransport } from '@/admin-api/client';
import { setTestLanguage } from '@/test/setup';
import { SettingsPage } from './SettingsPage';
import { formatDateTime } from '@/utils/format';

const secret = 'test-rotated-key';
type RotateResult = { old_id: number; new_id: number; key_prefix: string; key: string; overlap_until: string | null };
const baseKey: VirtualKey = {
  id: 9, name: 'editor', key_prefix: 'gw_test', key_recoverable: false,
  allowed_models: ['model-a'], enabled: true, created_at: '2026-09-01T00:00:00Z',
};

describe('Virtual Key rotation', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let keys: VirtualKey[];
  let overlapUntil: string | null;
  let capabilities: CapabilityMatrixResponse;
  let failCapabilitiesRefresh = false;
  let rotate: Mock<(init?: RequestInit) => Promise<RotateResult>>;
  let json: Mock<AdminTransport['json']>;

  beforeEach(async () => {
    await setTestLanguage('zh');
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
    keys = [{ ...baseKey }];
    overlapUntil = new Date(Date.now() + 3600000).toISOString();
    failCapabilitiesRefresh = false;
    capabilities = {
      version: 'v1', fact_source: 'runtime_snapshot', snapshot_revision: 1,
      snapshot_generated_at: baseKey.created_at, data: [],
    };
    rotate = vi.fn<(init?: RequestInit) => Promise<RotateResult>>(async () => {
      keys = [{ ...baseKey, replaced_by_id: 10, overlap_until: overlapUntil }, { ...baseKey, id: 10, key_recoverable: true }];
      return { old_id: 9, new_id: 10, key_prefix: 'gw_new', key: secret, overlap_until: overlapUntil };
    });
    json = vi.fn(async (path: string, init?: RequestInit) => {
      if (path === '/admin/keys') return { data: keys };
      if (path === '/admin/capabilities') {
        if (failCapabilitiesRefresh) throw { code: 'capabilities_refresh_failed', message: 'Synthetic capabilities refresh failure', status: 503 };
        return capabilities;
      }
      if (path === '/admin/config/reload') return {
        status: 'reloaded', snapshot_revision: 2, snapshot_generated_at: '2026-09-02T00:00:00Z',
      };
      if (path === '/admin/keys/9/rotate') return rotate(init);
      throw new Error(`Unexpected request ${path}`);
    }) as unknown as Mock<AdminTransport['json']>;
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  const render = async () => {
    await act(async () => root.render(<SettingsPage api={new GatewayAdminResources({ json: json as AdminTransport['json'] })} adminKeyConfigured onClearAdminKey={() => {}} />));
  };
  const button = (label: string) => {
    const result = Array.from(document.body.querySelectorAll('button')).find((el) => el.getAttribute('aria-label') === label || el.textContent === label);
    expect(result, `button ${label}`).toBeDefined();
    return result!;
  };
  const open = async () => {
    await render();
    await act(async () => {
      const trigger = button('轮换 editor');
      trigger.focus();
      trigger.click();
    });
  };
  const field = (selector: string) => document.querySelector<HTMLInputElement | HTMLTextAreaElement>(`#virtual-key-rotation-form ${selector}`)!;
  const setValue = async (selector: string, value: string) => {
    await act(async () => {
      const el = field(selector);
      const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(el, value);
      el.dispatchEvent(new Event('input', { bubbles: true }));
    });
  };
  const submit = async () => {
    await act(async () => button('轮换密钥').click());
  };
  const waitFor = async (condition: () => boolean) => {
    const deadline = Date.now() + 1000;
    while (!condition() && Date.now() < deadline) {
      await act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)); });
    }
    expect(condition()).toBe(true);
  };
  const waitForOverlayExit = async () => {
    await waitFor(() => document.body.textContent?.includes(secret) ?? false);
  };

  it('prefills the allowlist, rotates with edited models, reveals the new key and refreshes old-key state', async () => {
    await open();
    expect(field('textarea').value).toBe('model-a');
    expect(field('input').value).toBe('3600');
    await setValue('textarea', 'model-b, model-b, model-c, ');
    await submit();
    await waitForOverlayExit();
    expect(rotate).toHaveBeenCalledWith(expect.objectContaining({ method: 'POST', body: JSON.stringify({ overlap_secs: 3600, allowed_models: ['model-b', 'model-c'] }) }));
    expect(document.body.textContent).toContain(secret);
    expect(container.textContent).toContain('重叠期');
    expect(container.textContent).toContain('最晚有效至');
    expect(button('轮换 editor').getAttribute('aria-disabled')).toBe('true');
    expect(json.mock.calls.filter(([path]) => path === '/admin/keys')).toHaveLength(2);
  });

  it('supports immediate rotation and explicitly clearing the allowlist', async () => {
    overlapUntil = null;
    await open();
    await setValue('input', '0');
    await setValue('textarea', '');
    expect(document.body.textContent).toContain('旧密钥立即失效');
    await submit();
    await waitForOverlayExit();
    expect(rotate).toHaveBeenCalledWith(expect.objectContaining({ body: JSON.stringify({ overlap_secs: 0, allowed_models: [] }) }));
    expect(container.textContent).toContain('旧密钥已失效');
    expect(container.querySelector('[role="dialog"]')?.textContent).toContain('旧密钥已失效');
    expect(container.querySelector('.mantine-Notification-root')).toBeNull();
  });

  it('retains the earlier expiry in the result modal through copying and clears secrets on close', async () => {
    const expiry = new Date(Date.now() + 600000).toISOString();
    keys = [{ ...baseKey, expires_at: expiry }];
    const writeText = vi.fn().mockRejectedValueOnce(new Error('Clipboard denied')).mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
    await open();
    await submit();
    await waitForOverlayExit();
    const dialog = container.querySelector('[role="dialog"]')!;
    expect(dialog.textContent).toContain(formatDateTime(expiry));
    expect(dialog.textContent).not.toContain(formatDateTime(overlapUntil));
    expect(container.querySelector('.mantine-Notification-root')).toBeNull();
    await act(async () => button('复制 API Key').click());
    expect(dialog.textContent).toContain('复制');
    expect(dialog.textContent).toContain(secret);
    await act(async () => button('复制 API Key').click());
    expect(writeText).toHaveBeenLastCalledWith(secret);
    expect(dialog.textContent).toContain('API Key 已复制到剪贴板');
    expect(dialog.textContent).toContain(formatDateTime(expiry));
    expect(dialog.querySelector('[role="status"]')?.textContent).not.toContain(secret);
    await act(async () => button('完成').click());
    expect(document.body.textContent).not.toContain(secret);
    expect(document.body.textContent).not.toContain('API Key 已复制到剪贴板');
  });

  it.each(['', '-1', '1.5', '86401'])('rejects invalid overlap %j without a mutation', async (value) => {
    await open();
    await setValue('input', value);
    await submit();
    expect(rotate).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain('请输入 0–86400 之间的整数秒数');
  });

  it('accepts the maximum overlap and retains the form after a server error for retry', async () => {
    rotate.mockRejectedValueOnce({ code: 'key_conflict', message: 'key already rotated', status: 409 });
    await open();
    await setValue('input', '86400');
    await submit();
    expect(field('input').value).toBe('86400');
    expect(document.body.textContent).not.toContain(secret);
    expect(document.querySelector('#virtual-key-rotation-form [role="alert"]')).not.toBeNull();
    await submit();
    await waitForOverlayExit();
    expect(rotate).toHaveBeenCalledTimes(2);
    expect(document.body.textContent).toContain(secret);
  });

  it('blocks duplicate submissions and dismissal while rotation is pending', async () => {
    let finish!: (value: RotateResult) => void;
    rotate.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    await open();
    await submit();
    expect(button('轮换密钥').disabled).toBe(true);
    expect(button('取消').disabled).toBe(true);
    await submit();
    expect(rotate).toHaveBeenCalledTimes(1);
    await act(async () => finish({ old_id: 9, new_id: 10, key_prefix: 'gw_new', key: secret, overlap_until: null }));
  });

  it.each([
    { enabled: false }, { revoked_at: baseKey.created_at },
    { expires_at: baseKey.created_at }, { replaced_by_id: 10, overlap_until: new Date(Date.now() + 3600000).toISOString() },
  ])('disables rotation for an ineligible key %j', async (state) => {
    keys = [{ ...baseKey, ...state }];
    await render();
    expect(button('轮换 editor').getAttribute('aria-disabled')).toBe('true');
  });

  it('opens the result only after the rotation dialog exits and returns focus to the row action', async () => {
    await open();
    const trigger = button('轮换 editor');
    await submit();
    await waitForOverlayExit();
    const resultDialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(resultDialog.textContent).toContain(secret);
    await waitFor(() => resultDialog.contains(document.activeElement));
    expect(resultDialog.contains(document.activeElement)).toBe(true);
    await act(async () => button('完成').click());
    await waitFor(() => document.activeElement === trigger);
    expect(document.body.textContent).not.toContain(secret);
    expect(document.activeElement).toBe(trigger);
  });

  it('updates the status when the overlap ends without a manual refresh', async () => {
    vi.useFakeTimers();
    keys = [{ ...baseKey, replaced_by_id: 10, overlap_until: new Date(Date.now() + 500).toISOString() }];
    await render();
    expect(container.textContent).toContain('重叠期');
    await act(async () => vi.advanceTimersByTime(1000));
    expect(container.textContent).toContain('已轮换');
    expect(container.textContent).not.toContain('重叠期');
  });

  it('renders every snapshot field from the refreshed capabilities query after a reload', async () => {
    await render();
    await waitFor(() => container.textContent?.includes('运行时版本 1') ?? false);
    capabilities = {
      ...capabilities,
      snapshot_revision: 3,
      snapshot_generated_at: '2026-09-03T00:00:00Z',
      data: [{ route_id: 'route-a' }] as CapabilityMatrixResponse['data'],
    };

    await act(async () => button('重新加载运行时').click());
    await waitFor(() => container.textContent?.includes('运行时版本 3') ?? false);

    expect(container.textContent).toContain(formatDateTime(capabilities.snapshot_generated_at));
    expect(container.textContent).toContain('1 条能力条目');
    expect(container.textContent).not.toContain('运行时版本 2');
    expect(json.mock.calls.filter(([path]) => path === '/admin/capabilities')).toHaveLength(2);
  });

  it('keeps the published snapshot when post-reload refresh fails', async () => {
    await render();
    await waitFor(() => container.textContent?.includes('运行时版本 1') ?? false);
    failCapabilitiesRefresh = true;

    await act(async () => button('重新加载运行时').click());
    await waitFor(() => container.querySelector('[role="alert"]')?.textContent?.includes('capabilities_refresh_failed') ?? false);

    expect(container.textContent).toContain('运行时版本 1');
    expect(container.textContent).toContain(formatDateTime(baseKey.created_at));
    expect(container.textContent).not.toContain('运行时版本 2');
  });

  it('keeps each virtual-key timestamp as date and time lines', async () => {
    keys = [{ ...baseKey, last_used_at: '2026-09-02T12:34:56Z' }];
    await render();
    await waitFor(() => container.querySelector('tbody')?.textContent?.includes('editor') ?? false);

    const timestamps = container.querySelectorAll<HTMLTimeElement>('time');
    expect(timestamps).toHaveLength(2);
    for (const timestamp of timestamps) {
      expect(timestamp.dateTime).toMatch(/^2026-09/);
      expect(timestamp.querySelectorAll('span')).toHaveLength(2);
      expect(timestamp.textContent).toContain('2026');
    }
  });
});
