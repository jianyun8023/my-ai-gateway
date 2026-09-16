import { describe, expect, it, vi } from 'vitest';
import { UpstreamQuotaClient } from './client';
import type { UpstreamQuotaSnapshot } from './types';

const snapshot: UpstreamQuotaSnapshot = {
  account: {
    account_id: 'kimi/main',
    account_display_name: 'Kimi Main',
    source_id: 'kimi',
    source_display_name: 'Kimi Code CN',
    provider_id: 'kimi_code',
    enabled: true,
  },
  status: 'ok',
  resources: [],
  fetched_at: '2026-09-16T10:00:00Z',
  attempted_at: '2026-09-16T10:00:00Z',
  latency_ms: 100,
  stale: false,
  refresh_error: null,
  raw: null,
};

describe('UpstreamQuotaClient', () => {
  it('reads the list envelope', async () => {
    const json = vi.fn().mockResolvedValue({ data: [snapshot] });
    const client = new UpstreamQuotaClient({ json });
    await expect(client.list()).resolves.toEqual([snapshot]);
    expect(json).toHaveBeenCalledWith('/admin/upstream-quotas', { signal: undefined });
  });

  it('encodes account ids for detail and refresh endpoints', async () => {
    const json = vi.fn().mockResolvedValue({ data: snapshot });
    const client = new UpstreamQuotaClient({ json });
    await client.get('kimi/main');
    await client.refresh('kimi/main');
    expect(json).toHaveBeenNthCalledWith(1, '/admin/upstream-quotas/kimi%2Fmain', { signal: undefined });
    expect(json).toHaveBeenNthCalledWith(2, '/admin/upstream-quotas/kimi%2Fmain/refresh', { method: 'POST', signal: undefined });
  });

  it('refreshes all accounts with POST', async () => {
    const json = vi.fn().mockResolvedValue({ data: [snapshot] });
    const client = new UpstreamQuotaClient({ json });
    await expect(client.refreshAll()).resolves.toEqual([snapshot]);
    expect(json).toHaveBeenCalledWith('/admin/upstream-quotas/refresh', { method: 'POST', signal: undefined });
  });
});
