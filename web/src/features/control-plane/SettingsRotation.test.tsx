// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { GatewayAdminResources, type VirtualKey } from '@/admin-api';
import { setTestLanguage } from '@/test/setup';
import { SettingsPage } from './SettingsPage';

const secret = 'test-rotated-key';
const baseKey: VirtualKey = {
  id: 9, name: 'editor', key_prefix: 'gw_test', key_recoverable: false,
  allowed_models: ['model-a'], enabled: true, created_at: '2026-09-01T00:00:00Z',
};

describe('Virtual Key rotation', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let keys: VirtualKey[];
  let overlapUntil: string | null;
  let rotate: ReturnType<typeof vi.fn>;
  let json: ReturnType<typeof vi.fn>;

  beforeEach(async () => {
    await setTestLanguage('zh');
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
    keys = [{ ...baseKey }];
    overlapUntil = new Date(Date.now() + 3600000).toISOString();
    rotate = vi.fn(async () => {
      keys = [{ ...baseKey, replaced_by_id: 10, overlap_until: overlapUntil }, { ...baseKey, id: 10, key_recoverable: true }];
      return { old_id: 9, new_id: 10, key_prefix: 'gw_new', key: secret, overlap_until: overlapUntil };
    });
    json = vi.fn(async (path: string, init?: RequestInit) => {
      if (path === '/admin/keys') return { data: keys };
      if (path === '/admin/capabilities') return { data: [], fact_source: 'runtime_snapshot', snapshot_revision: 1, snapshot_generated_at: baseKey.created_at };
      if (path === '/admin/keys/9/rotate') return rotate(init);
      throw new Error(`Unexpected request ${path}`);
    });
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  const render = async () => {
    await act(async () => root.render(<SettingsPage api={new GatewayAdminResources({ json })} adminKeyConfigured onClearAdminKey={() => {}} />));
  };
  const button = (label: string) => {
    const result = Array.from(document.body.querySelectorAll('button')).find((el) => el.getAttribute('aria-label') === label || el.textContent === label);
    expect(result, `button ${label}`).toBeDefined();
    return result!;
  };
  const open = async () => {
    await render();
    await act(async () => button('轮换 editor').click());
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
    await act(async () => document.querySelector('#virtual-key-rotation-form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
  };

  it('prefills the allowlist, rotates with edited models, reveals the new key and refreshes old-key state', async () => {
    await open();
    expect(field('textarea').value).toBe('model-a');
    expect(field('input').value).toBe('3600');
    await setValue('textarea', 'model-b, model-b, model-c, ');
    await submit();
    expect(rotate).toHaveBeenCalledWith(expect.objectContaining({ method: 'POST', body: JSON.stringify({ overlap_secs: 3600, allowed_models: ['model-b', 'model-c'] }) }));
    expect(document.body.textContent).toContain(secret);
    expect(container.textContent).toContain('重叠期');
    expect(container.textContent).toContain('最晚有效至');
    expect(button('轮换 editor').disabled).toBe(true);
    expect(json.mock.calls.filter(([path]) => path === '/admin/keys')).toHaveLength(2);
  });

  it('supports immediate rotation and explicitly clearing the allowlist', async () => {
    overlapUntil = null;
    await open();
    await setValue('input', '0');
    await setValue('textarea', '');
    expect(document.body.textContent).toContain('旧密钥立即失效');
    await submit();
    expect(rotate).toHaveBeenCalledWith(expect.objectContaining({ body: JSON.stringify({ overlap_secs: 0, allowed_models: [] }) }));
    expect(container.textContent).toContain('旧密钥已失效');
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
    expect(rotate).toHaveBeenCalledTimes(2);
    expect(document.body.textContent).toContain(secret);
  });

  it('blocks duplicate submissions and dismissal while rotation is pending', async () => {
    let finish!: (value: unknown) => void;
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
    expect(button('轮换 editor').disabled).toBe(true);
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
});
