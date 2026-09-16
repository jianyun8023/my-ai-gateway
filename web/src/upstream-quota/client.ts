import type { AdminTransport } from '@/admin-api/client';
import type { UpstreamQuotaSnapshot } from './types';

interface DataEnvelope<T> {
  data: T;
}

interface SessionStorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

const CACHE_PREFIX = 'my-ai-gateway:upstream-quota:v1:';
const FAILED_STATUSES = new Set(['refresh_failed', 'auth_error']);
const encodePath = (value: string): string => encodeURIComponent(value);
const cacheKey = (accountId: string): string => `${CACHE_PREFIX}${encodePath(accountId)}`;

const sessionStorageIfAvailable = (): SessionStorageLike | undefined => {
  try {
    return typeof globalThis.sessionStorage === 'undefined' ? undefined : globalThis.sessionStorage;
  } catch {
    return undefined;
  }
};

const cachedSnapshot = (
  storage: SessionStorageLike | undefined,
  accountId: string,
): UpstreamQuotaSnapshot | undefined => {
  if (!storage) return undefined;
  try {
    const raw = storage.getItem(cacheKey(accountId));
    if (!raw) return undefined;
    const parsed = JSON.parse(raw) as UpstreamQuotaSnapshot;
    if (
      parsed?.account?.account_id !== accountId
      || !Array.isArray(parsed.resources)
      || typeof parsed.status !== 'string'
    ) return undefined;
    return parsed;
  } catch {
    return undefined;
  }
};

const rememberSnapshot = (
  storage: SessionStorageLike | undefined,
  snapshot: UpstreamQuotaSnapshot,
): void => {
  if (
    !storage
    || FAILED_STATUSES.has(snapshot.status)
    || snapshot.resources.length === 0
    || !snapshot.fetched_at
  ) return;

  try {
    storage.setItem(cacheKey(snapshot.account.account_id), JSON.stringify({
      ...snapshot,
      stale: false,
      refresh_error: null,
      raw: undefined,
    }));
  } catch {
    // Quota display must not fail because sessionStorage is disabled/full.
  }
};

const mergeCachedSnapshot = (
  storage: SessionStorageLike | undefined,
  next: UpstreamQuotaSnapshot,
): UpstreamQuotaSnapshot => {
  const previous = cachedSnapshot(storage, next.account.account_id);
  if (
    previous
    && previous.resources.length > 0
    && next.resources.length === 0
    && FAILED_STATUSES.has(next.status)
  ) {
    return {
      ...next,
      resources: previous.resources,
      fetched_at: previous.fetched_at,
      stale: true,
    };
  }
  return next;
};

export class UpstreamQuotaClient {
  private readonly storage: SessionStorageLike | undefined;

  constructor(
    private readonly transport: Pick<AdminTransport, 'json'>,
    storage: SessionStorageLike | undefined = sessionStorageIfAvailable(),
  ) {
    this.storage = storage;
  }

  private applySessionSnapshot(next: UpstreamQuotaSnapshot): UpstreamQuotaSnapshot {
    const merged = mergeCachedSnapshot(this.storage, next);
    rememberSnapshot(this.storage, next);
    return merged;
  }

  private applySessionSnapshots(next: UpstreamQuotaSnapshot[]): UpstreamQuotaSnapshot[] {
    return next.map((snapshot) => this.applySessionSnapshot(snapshot));
  }

  async list(signal?: AbortSignal): Promise<UpstreamQuotaSnapshot[]> {
    const data = (await this.transport.json<DataEnvelope<UpstreamQuotaSnapshot[]>>(
      '/admin/upstream-quotas',
      { signal },
    )).data;
    return this.applySessionSnapshots(data);
  }

  async get(accountId: string, signal?: AbortSignal): Promise<UpstreamQuotaSnapshot> {
    const data = (await this.transport.json<DataEnvelope<UpstreamQuotaSnapshot>>(
      `/admin/upstream-quotas/${encodePath(accountId)}`,
      { signal },
    )).data;
    return this.applySessionSnapshot(data);
  }

  async refreshAll(signal?: AbortSignal): Promise<UpstreamQuotaSnapshot[]> {
    const data = (await this.transport.json<DataEnvelope<UpstreamQuotaSnapshot[]>>(
      '/admin/upstream-quotas/refresh',
      { method: 'POST', signal },
    )).data;
    return this.applySessionSnapshots(data);
  }

  async refresh(accountId: string, signal?: AbortSignal): Promise<UpstreamQuotaSnapshot> {
    const data = (await this.transport.json<DataEnvelope<UpstreamQuotaSnapshot>>(
      `/admin/upstream-quotas/${encodePath(accountId)}/refresh`,
      { method: 'POST', signal },
    )).data;
    return this.applySessionSnapshot(data);
  }
}
