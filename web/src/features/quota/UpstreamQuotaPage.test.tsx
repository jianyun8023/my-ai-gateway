// @vitest-environment happy-dom
import { act } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createRoot } from '@/test/render';
import { setTestLanguage } from '@/test/setup';
import type { AccountHealthView, GatewayAdminResources } from '@/admin-api';
import type { UpstreamQuotaClient } from '@/upstream-quota/client';
import type { UpstreamQuotaSnapshot } from '@/upstream-quota/types';
import { UpstreamQuotaPage } from './UpstreamQuotaPage';

const account = {
  account_id: 'kimi-main',
  account_display_name: 'Kimi Code 主账号',
  source_id: 'kimi-code-cn',
  source_display_name: 'Kimi Code CN',
  provider_id: 'kimi_code',
  enabled: true,
};

const kimiSnapshot: UpstreamQuotaSnapshot = {
  account,
  status: 'exhausted',
  resources: [
    { type: 'window', key: '5h', label: '5 小时', unit: 'percent', remaining: 100, used: 0, limit: 100, reset_at: '2026-09-17T11:03:11Z' },
    { type: 'window', key: '7d', label: '7 天', unit: 'percent', remaining: 0, used: 100, limit: 100, reset_at: '2026-09-17T23:03:11Z' },
  ],
  fetched_at: '2026-09-17T07:50:16Z',
  attempted_at: '2026-09-17T07:50:16Z',
  latency_ms: 145,
  stale: false,
};

const deepSeekSnapshot: UpstreamQuotaSnapshot = {
  account: {
    account_id: 'deepseek-main',
    account_display_name: 'DeepSeek 主账号',
    source_id: 'deepseek',
    source_display_name: 'DeepSeek',
    provider_id: 'deepseek',
    enabled: true,
  },
  status: 'ok',
  resources: [
    { type: 'balance', key: 'balance_cny', label: 'CNY 余额', unit: 'CNY', remaining: 11.2 },
  ],
  fetched_at: '2026-09-17T07:50:16Z',
  attempted_at: '2026-09-17T07:50:16Z',
  latency_ms: 120,
  stale: false,
};

async function waitFor(condition: () => boolean) {
  const deadline = Date.now() + 1000;
  while (!condition() && Date.now() < deadline) {
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)); });
  }
  expect(condition()).toBe(true);
}

describe('UpstreamQuotaPage', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(async () => {
    await setTestLanguage('zh');
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.restoreAllMocks();
  });

  const renderPage = async (items: UpstreamQuotaSnapshot[], health: AccountHealthView[] = []) => {
    const client = {
      list: vi.fn(async () => items),
      refreshAll: vi.fn(async () => items),
      refresh: vi.fn(async (accountId: string) => items.find((item) => item.account.account_id === accountId)!),
    } as unknown as UpstreamQuotaClient;
    const api = { health: vi.fn(async () => ({ fact_source: 'postgresql', stale_after_secs: 600, data: health })) } as unknown as GatewayAdminResources;
    await act(async () => root.render(<UpstreamQuotaPage client={client} api={api} />));
    await waitFor(() => container.querySelectorAll('tbody tr').length === items.length);
  };

  it('attributes exhausted quota to the 7-day window only when the snapshot provides that window', async () => {
    await renderPage([deepSeekSnapshot, kimiSnapshot]);

    const kimiRow = [...container.querySelectorAll('tbody tr')]
      .find((row) => row.textContent?.includes('Kimi Code 主账号'))!;
    expect(kimiRow.textContent).toContain('已耗尽');
    expect(kimiRow.textContent).toContain('该快照中的 7 天窗口 已无剩余额度');
    expect(kimiRow.textContent).not.toContain('由上游额度接口判定');

    const alertCard = [...container.querySelectorAll<HTMLElement>('[data-ui="metric-card"]')]
      .find((card) => card.textContent?.includes('额度告警'))!;
    expect(alertCard.querySelector('strong')?.textContent).toBe('1');
    expect(container.querySelector('thead')?.textContent).toContain('提供商');
    expect(container.querySelector('thead')?.textContent).not.toContain('PROVIDER');
  });

  it('keeps exhausted status at the upstream level when no exhausted window was returned', async () => {
    await renderPage([{
      ...kimiSnapshot,
      resources: [],
    }]);

    const row = container.querySelector('tbody tr')!;
    expect(row.textContent).toContain('由上游额度接口判定');
    expect(row.textContent).not.toContain('7 天窗口 已无剩余额度');
  });

  it('marks retained data as the last successful snapshot after an update failure', async () => {
    await renderPage([{
      ...kimiSnapshot,
      status: 'refresh_failed',
      stale: true,
      refresh_error: { code: 'upstream_timeout', message: 'request timed out' },
    }]);

    const row = container.querySelector('tbody tr')!;
    expect(row.textContent).toContain('快照已过期');
    expect(row.textContent).toContain('最近成功快照');
    expect(row.textContent).toContain('本次刷新：刷新失败');
    expect(row.textContent).not.toContain('时为“刷新失败”');
    expect(container.textContent).toContain('保留的成功快照不视为当前额度');
  });

  it('does not count stale normal quota as fresh while retaining an independently fresh health record', async () => {
    await renderPage([{
      ...deepSeekSnapshot,
      status: 'ok',
      stale: true,
      refresh_error: { code: 'upstream_timeout', message: 'request timed out' },
    }], [{
      account_id: 'deepseek-main', source_id: 'deepseek', display_name: 'DeepSeek 主账号', enabled: true,
      health_status: 'healthy', health_updated_at: '2026-09-17T07:50:16Z', stale: false, cooldown_until: null,
      health: {
        status: 'healthy', stale: false, updated_at: '2026-09-17T07:50:16Z', last_probe_at: null,
        cooldown_until: null, last_error: null, last_success_at: '2026-09-17T07:50:16Z',
      },
    }]);

    const cards = [...container.querySelectorAll<HTMLElement>('[data-ui="metric-card"]')];
    const freshQuota = cards.find((card) => card.textContent?.includes('最新额度'))!;
    const stale = cards.find((card) => card.textContent?.includes('陈旧快照'))!;
    expect(freshQuota.querySelector('strong')?.textContent).toBe('0');
    expect(stale.querySelector('strong')?.textContent).toBe('1');
    expect(container.textContent).toContain('健康');
    expect(container.textContent).toContain('记录于');
  });

  it('does not show stale routing health as healthy even with a fresh quota snapshot', async () => {
    await renderPage([deepSeekSnapshot], [{
      account_id: 'deepseek-main', source_id: 'deepseek', display_name: 'DeepSeek 主账号', enabled: true,
      health_status: 'stale', health_updated_at: '2026-09-17T07:10:16Z', stale: true, cooldown_until: null,
      health: {
        status: 'stale', stale: true, updated_at: '2026-09-17T07:10:16Z', last_probe_at: null,
        cooldown_until: null, last_error: 'last probe timed out', last_success_at: null,
      },
    }]);

    expect(container.textContent).toContain('健康状态待复核');
  });

  it('treats a 403 quota-query failure as neutral evidence instead of quota exhaustion', async () => {
    await renderPage([{
      ...deepSeekSnapshot,
      status: 'auth_error',
      resources: [],
      fetched_at: null,
      refresh_error: { code: 'upstream_auth_failed', message: 'upstream rejected the account credential', http_status: 403 },
    }]);

    const row = container.querySelector('tbody tr')!;
    expect(row.textContent).toContain('额度查询被拒绝');
    expect(row.textContent).toContain('单凭此结果不能判定额度耗尽');
    expect(row.textContent).not.toContain('已耗尽');
  });

  it('keeps a first-load failure actionable instead of rendering empty metrics', async () => {
    const list = vi.fn()
      .mockRejectedValueOnce(new Error('quota service unavailable'))
      .mockResolvedValueOnce([deepSeekSnapshot]);
    const client = {
      list,
      refreshAll: vi.fn(),
      refresh: vi.fn(),
    } as unknown as UpstreamQuotaClient;

    await act(async () => root.render(<UpstreamQuotaPage client={client} api={{ health: vi.fn(async () => ({ data: [] })) } as unknown as GatewayAdminResources} />));
    await waitFor(() => container.textContent?.includes('获取上游额度失败') ?? false);
    expect(container.textContent).toContain('quota service unavailable');
    expect(container.querySelector('[data-ui="metric-card"]')).toBeNull();

    const retry = [...container.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.includes('重试'))!;
    await act(async () => retry.click());
    await waitFor(() => container.querySelectorAll('tbody tr').length === 1);
    expect(list).toHaveBeenCalledTimes(2);
  });

  it('uses separate empty states for missing accounts and filtered-out accounts', async () => {
    const noAccountsClient = {
      list: vi.fn(async () => []),
      refreshAll: vi.fn(),
      refresh: vi.fn(),
    } as unknown as UpstreamQuotaClient;
    await act(async () => root.render(<UpstreamQuotaPage client={noAccountsClient} api={{ health: vi.fn(async () => ({ data: [] })) } as unknown as GatewayAdminResources} />));
    await waitFor(() => container.textContent?.includes('尚未配置可查看额度的账号') ?? false);
    expect(container.textContent).toContain('配置账号');
    const configureAccounts = [...container.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.includes('配置账号'))!;
    await act(async () => configureAccounts.click());
    expect(window.location.hash).toBe('#sources');

    await act(async () => root.render(<UpstreamQuotaPage client={{
      list: vi.fn(async () => [deepSeekSnapshot]),
      refreshAll: vi.fn(),
      refresh: vi.fn(),
    } as unknown as UpstreamQuotaClient} api={{ health: vi.fn(async () => ({ data: [] })) } as unknown as GatewayAdminResources} />));
    await waitFor(() => container.querySelectorAll('tbody tr').length === 1);

    const onlyProblems = container.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    await act(async () => onlyProblems.click());
    await waitFor(() => container.textContent?.includes('没有符合筛选条件的账号') ?? false);

    const reset = [...container.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.includes('重置筛选'))!;
    await act(async () => reset.click());
    await waitFor(() => container.querySelectorAll('tbody tr').length === 1);
  });

  it('keeps a detail fetch failure retryable with a back action', async () => {
    const get = vi.fn()
      .mockRejectedValueOnce(new Error('detail unavailable'))
      .mockResolvedValueOnce(kimiSnapshot);
    const client = {
      get,
      list: vi.fn(),
      refreshAll: vi.fn(),
      refresh: vi.fn(),
    } as unknown as UpstreamQuotaClient;

    await act(async () => root.render(<UpstreamQuotaPage client={client} api={{ health: vi.fn(async () => ({ data: [] })) } as unknown as GatewayAdminResources} accountId="kimi-main" />));
    await waitFor(() => container.textContent?.includes('获取上游额度失败') ?? false);
    expect(container.textContent).toContain('返回上游额度');

    const retry = [...container.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.includes('重试'))!;
    await act(async () => retry.click());
    await waitFor(() => container.textContent?.includes('Kimi Code 主账号') ?? false);
    expect(get).toHaveBeenCalledTimes(2);
  });
});
