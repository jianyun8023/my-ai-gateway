import type { AdminTransport } from '@/admin-api/client';
import type { UpstreamQuotaSnapshot } from './types';

interface DataEnvelope<T> {
  data: T;
}

const encodePath = (value: string): string => encodeURIComponent(value);

export class UpstreamQuotaClient {
  constructor(private readonly transport: Pick<AdminTransport, 'json'>) {}

  async list(signal?: AbortSignal): Promise<UpstreamQuotaSnapshot[]> {
    return (await this.transport.json<DataEnvelope<UpstreamQuotaSnapshot[]>>(
      '/admin/upstream-quotas',
      { signal },
    )).data;
  }

  async get(accountId: string, signal?: AbortSignal): Promise<UpstreamQuotaSnapshot> {
    return (await this.transport.json<DataEnvelope<UpstreamQuotaSnapshot>>(
      `/admin/upstream-quotas/${encodePath(accountId)}`,
      { signal },
    )).data;
  }

  async refreshAll(signal?: AbortSignal): Promise<UpstreamQuotaSnapshot[]> {
    return (await this.transport.json<DataEnvelope<UpstreamQuotaSnapshot[]>>(
      '/admin/upstream-quotas/refresh',
      { method: 'POST', signal },
    )).data;
  }

  async refresh(accountId: string, signal?: AbortSignal): Promise<UpstreamQuotaSnapshot> {
    return (await this.transport.json<DataEnvelope<UpstreamQuotaSnapshot>>(
      `/admin/upstream-quotas/${encodePath(accountId)}/refresh`,
      { method: 'POST', signal },
    )).data;
  }
}
