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

const memoryStorage = () => {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
  };
};

describe('UpstreamQuotaClient', () => {
  it('reads the list envelope', async () => {
    const json = vi.fn().mockResolvedValue({ data: [snapshot] });
    const client = new UpstreamQuotaClient({ json }, memoryStorage());
    await expect(client.list()).resolves.toEqual([snapshot]);
    expect(json).toHaveBeenCalledWith('/admin/upstream-quotas', { signal: undefined });
  });

  it('encodes account ids for detail and refresh endpoints', async () => {
    const json = vi.fn().mockResolvedValue({ data: snapshot });
    const client = new UpstreamQuotaClient({ json }, memoryStorage());
    await client.get('kimi/main');
    await client.refresh('kimi/main');
    expect(json).toHaveBeenNthCalledWith(1, '/admin/upstream-quotas/kimi%2Fmain', { signal: undefined });
    expect(json).toHaveBeenNthCalledWith(2, '/admin/upstream-quotas/kimi%2Fmain/refresh', { method: 'POST', signal: undefined });
  });

  it('refreshes all accounts with POST', async () => {
    const json = vi.fn().mockResolvedValue({ data: [snapshot] });
    const client = new UpstreamQuotaClient({ json }, memoryStorage());
    await expect(client.refreshAll()).resolves.toEqual([snapshot]);
    expect(json).toHaveBeenCalledWith('/admin/upstream-quotas/refresh', { method: 'POST', signal: undefined });
  });

  it('keeps the last successful resource snapshot across client instances in the same browser session', async () => {
    const storage = memoryStorage();
    const successful: UpstreamQuotaSnapshot = {
      ...snapshot,
      resources: [{
        type: 'window',
        key: '5h',
        label: '5 小时',
        unit: 'percent',
        used: 25,
        remaining: 75,
        limit: 100,
        reset_at: '2026-09-16T12:00:00Z',
      }],
      raw: { user: { id: 'must-not-be-cached' } },
    };
    const firstJson = vi.fn().mockResolvedValue({ data: successful });
    await new UpstreamQuotaClient({ json: firstJson }, storage).get('kimi/main');

    const failed: UpstreamQuotaSnapshot = {
      ...snapshot,
      status: 'refresh_failed',
      resources: [],
      fetched_at: null,
      attempted_at: '2026-09-16T10:05:00Z',
      refresh_error: { code: 'upstream_timeout', message: 'timed out' },
      raw: null,
    };
    const secondJson = vi.fn().mockResolvedValue({ data: failed });
    const restored = await new UpstreamQuotaClient({ json: secondJson }, storage).get('kimi/main');

    expect(restored.status).toBe('refresh_failed');
    expect(restored.stale).toBe(true);
    expect(restored.resources).toEqual(successful.resources);
    expect(restored.fetched_at).toBe(successful.fetched_at);
    expect(restored.raw).toBeNull();
    expect([...storageValues(storage)].join(' ')).not.toContain('must-not-be-cached');
  });
});

function storageValues(storage: ReturnType<typeof memoryStorage>): string[] {
  const probeKeys = ['my-ai-gateway:upstream-quota:v1:kimi%2Fmain'];
  return probeKeys.map((key) => storage.getItem(key) ?? '');
}
