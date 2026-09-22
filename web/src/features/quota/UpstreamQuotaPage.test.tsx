// @vitest-environment happy-dom
import { act } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createRoot } from '@/test/render';
import { setTestLanguage } from '@/test/setup';
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

  const renderPage = async (items: UpstreamQuotaSnapshot[]) => {
    const client = {
      list: vi.fn(async () => items),
      refreshAll: vi.fn(async () => items),
      refresh: vi.fn(async (accountId: string) => items.find((item) => item.account.account_id === accountId)!),
    } as unknown as UpstreamQuotaClient;
    await act(async () => root.render(<UpstreamQuotaPage client={client} />));
    await waitFor(() => container.querySelectorAll('tbody tr').length === items.length);
  };

  it('keeps the API exhausted status without attributing it to a visible quota window', async () => {
    await renderPage([deepSeekSnapshot, kimiSnapshot]);

    const kimiRow = [...container.querySelectorAll('tbody tr')]
      .find((row) => row.textContent?.includes('Kimi Code 主账号'))!;
    expect(kimiRow.textContent).toContain('已耗尽');
    expect(kimiRow.textContent).toContain('由上游额度接口判定');
    expect(kimiRow.textContent).not.toContain('7 天额度耗尽');

    const alertCard = [...container.querySelectorAll<HTMLElement>('[data-ui="metric-card"]')]
      .find((card) => card.textContent?.includes('额度告警'))!;
    expect(alertCard.querySelector('strong')?.textContent).toBe('1');
    expect(container.querySelector('thead')?.textContent).toContain('提供商');
    expect(container.querySelector('thead')?.textContent).not.toContain('PROVIDER');
  });

  it('marks retained data as the last successful snapshot after an update failure', async () => {
    await renderPage([{
      ...kimiSnapshot,
      status: 'refresh_failed',
      stale: true,
      refresh_error: { code: 'upstream_timeout', message: 'request timed out' },
    }]);

    const row = container.querySelector('tbody tr')!;
    expect(row.textContent).toContain('刷新失败');
    expect(row.textContent).toContain('成功快照');
    expect(container.textContent).toContain('刷新失败时会保留最近一次成功数据');
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

    await act(async () => root.render(<UpstreamQuotaPage client={client} />));
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
    await act(async () => root.render(<UpstreamQuotaPage client={noAccountsClient} />));
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
    } as unknown as UpstreamQuotaClient} />));
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

    await act(async () => root.render(<UpstreamQuotaPage client={client} accountId="kimi-main" />));
    await waitFor(() => container.textContent?.includes('获取上游额度失败') ?? false);
    expect(container.textContent).toContain('返回上游额度');

    const retry = [...container.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.includes('重试'))!;
    await act(async () => retry.click());
    await waitFor(() => container.textContent?.includes('Kimi Code 主账号') ?? false);
    expect(get).toHaveBeenCalledTimes(2);
  });
});
