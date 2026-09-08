import { describe, expect, it, vi } from 'vitest';
import { GatewayAdminResources } from './resources';
import type {
  Account,
  CapabilityMatrixResponse,
  LogicalModel,
  ModelBinding,
  Route,
  Source,
} from './types';

const transportWith = (handler: (path: string, init?: RequestInit) => Promise<unknown>) => {
  const transport = { async json<T>(path: string, init?: RequestInit): Promise<T> { return await handler(path, init) as T; } };
  vi.spyOn(transport, 'json');
  return transport;
};

describe('GatewayAdminResources', () => {
  it('sends typed source mutations to encoded Admin resource paths', async () => {
    const transport = transportWith(async () => ({
      data: {},
      snapshot_revision: 2,
      snapshot_generated_at: '2026-08-31T00:00:00Z',
    }));
    const api = new GatewayAdminResources(transport);

    await api.setSourceEnabled('source/with space', false);

    expect(transport.json).toHaveBeenCalledWith(
      '/admin/sources/source%2Fwith%20space/enabled',
      expect.objectContaining({
        method: 'PUT',
        body: JSON.stringify({ enabled: false }),
      }),
    );
  });

  it('keeps the three-protocol connection test contract intact', async () => {
    const controller = new AbortController();
    const transport = transportWith(async () => ({ data: { status: 'succeeded' } }));
    const api = new GatewayAdminResources(transport);

    await api.testConnection('source-a', {
      account_id: 'account-a',
      protocol: 'openai_responses',
      requested_by: 'admin-ui',
    }, controller.signal);

    expect(transport.json).toHaveBeenCalledWith(
      '/admin/sources/source-a/connection-tests',
      expect.objectContaining({
        method: 'POST',
        signal: controller.signal,
        body: JSON.stringify({
          account_id: 'account-a',
          protocol: 'openai_responses',
          requested_by: 'admin-ui',
        }),
      }),
    );
  });

  it('treats only the structured discovery_not_found response as an empty latest run', async () => {
    const missing = Object.assign(new Error('no discovery'), {
      status: 404,
      code: 'discovery_not_found',
    });
    const transport = transportWith(async () => { throw missing; });

    await expect(new GatewayAdminResources(transport).latestDiscovery('source-a')).resolves.toBeNull();

    const other = Object.assign(new Error('source missing'), { status: 404, code: 'not_found' });
    const failingTransport = transportWith(async () => { throw other; });
    await expect(new GatewayAdminResources(failingTransport).latestDiscovery('source-a')).rejects.toBe(other);
  });

  it('passes pending and availability filters without inventing model state', async () => {
    const transport = transportWith(async () => ({ data: [] }));
    const api = new GatewayAdminResources(transport);

    await api.sourceModels('source-a', {
      confirmationStatus: 'pending',
      availabilityStatus: 'unknown',
    });

    expect(transport.json).toHaveBeenCalledWith(
      '/admin/sources/source-a/models?confirmation_status=pending&availability_status=unknown',
      expect.any(Object),
    );
  });

  it('lists and upserts source model capabilities on encoded nested paths', async () => {
    const transport = transportWith(async () => ({
      data: { protocol: 'openai_responses' },
      snapshot_revision: 5,
      snapshot_generated_at: '2026-09-06T00:00:00Z',
    }));
    const api = new GatewayAdminResources(transport);

    await api.sourceModelCapabilities('source-a', 'model/with space');
    expect(transport.json).toHaveBeenCalledWith(
      '/admin/sources/source-a/models/model%2Fwith%20space/capabilities',
      expect.any(Object),
    );

    await api.upsertSourceModelCapability('source-a', 'model-a', 'openai_responses', {
      status: 'confirmed',
      mode: 'adapter',
      source_protocol: 'anthropic_messages',
      adapter: 'example_adapter',
    });
    expect(transport.json).toHaveBeenCalledWith(
      '/admin/sources/source-a/models/model-a/capabilities/openai_responses',
      expect.objectContaining({
        method: 'PUT',
        body: JSON.stringify({
          status: 'confirmed',
          mode: 'adapter',
          source_protocol: 'anthropic_messages',
          adapter: 'example_adapter',
        }),
      }),
    );
  });

  it('uses the explicit secret endpoint to reveal one Virtual Key', async () => {
    const transport = transportWith(async () => ({ data: { id: 7, key: 'gw_secret' } }));

    await expect(new GatewayAdminResources(transport).revealVirtualKey(7)).resolves.toEqual({
      id: 7,
      key: 'gw_secret',
    });
    expect(transport.json).toHaveBeenCalledWith('/admin/keys/7/value', expect.any(Object));
  });

  it('builds configuration export from real resources and removes credential references', async () => {
    const source = {
      id: 'source-a',
      display_name: 'Source A',
    } as Source;
    const account = {
      id: 'account-a',
      source_id: 'source-a',
      display_name: 'Account A',
      credential_env: 'PROVIDER_KEY_ENV',
      credential_configured: true,
    } as Account;
    const logicalModel = { id: 'model-a' } as LogicalModel;
    const binding = { id: 1 } as ModelBinding;
    const route = { id: 'route-a' } as Route;
    const capability = {
      version: 'v1',
      fact_source: 'runtime_snapshot',
      snapshot_revision: 9,
      snapshot_generated_at: '2026-08-31T00:00:00Z',
      data: [],
    } as CapabilityMatrixResponse;
    const responses = new Map<string, unknown>([
      ['/admin/sources', { data: [source] }],
      ['/admin/accounts', { data: [account] }],
      ['/admin/logical-models', { data: [logicalModel] }],
      ['/admin/model-bindings', { data: [binding] }],
      ['/admin/routes', { data: [route] }],
      ['/admin/capabilities', capability],
    ]);
    const transport = transportWith(async (path) => responses.get(path));

    const exported = await new GatewayAdminResources(transport).sanitizedConfigurationExport();

    expect(exported.snapshot).toEqual({
      revision: 9,
      generated_at: '2026-08-31T00:00:00Z',
    });
    expect(exported.accounts[0]).not.toHaveProperty('credential_env');
    expect(exported.accounts[0]).toMatchObject({ credential_configured: true });
  });
});
