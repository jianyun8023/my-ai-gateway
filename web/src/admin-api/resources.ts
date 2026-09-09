import type {
  Account,
  AccountWriteInput,
  AdminDataEnvelope,
  AdminMutationEnvelope,
  CapabilityMatrixResponse,
  ConnectionTestInput,
  ConnectionTestResult,
  DiscoveryExecution,
  GatewayProtocol,
  FilterOptionsRange,
  FilterOptionsResponse,
  LatestDiscovery,
  LogicalModel,
  LogicalModelWriteInput,
  ModelBinding,
  ModelBindingWriteInput,
  ProviderPreset,
  ProviderPresetDiff,
  Route,
  RouteWriteInput,
  RuntimeEventFilters,
  RuntimeEventResponse,
  RuntimeReloadResult,
  SanitizedConfigurationExport,
  Source,
  SourceCreateInput,
  SourceModel,
  SourceModelCapability,
  SourceModelCapabilityWrite,
  SourceModelConfirmation,
  SourceModelEditInput,
  SourceModelFilters,
  SourceWriteInput,
  VirtualKey,
  VirtualKeyCreateInput,
  VirtualKeyCreateResult,
  VirtualKeyRotateInput,
  VirtualKeyRotateResult,
  VirtualKeySecret
} from './types';

import type { AdminTransport } from './client';
import { normalizeAdminError } from './errors';

const encodePath = (value: string | number): string => encodeURIComponent(String(value));

const jsonInit = (method: string, body?: unknown, signal?: AbortSignal): RequestInit => ({
  method,
  signal,
  headers: body === undefined ? undefined : { 'Content-Type': 'application/json' },
  body: body === undefined ? undefined : JSON.stringify(body),
});

export class GatewayAdminResources {
  constructor(private readonly transport: Pick<AdminTransport, 'json'>) {}

  async providerPresets(signal?: AbortSignal): Promise<ProviderPreset[]> {
    return (await this.transport.json<AdminDataEnvelope<ProviderPreset[]>>(
      '/admin/provider-presets',
      { signal },
    )).data;
  }

  async sources(signal?: AbortSignal): Promise<Source[]> {
    return (await this.transport.json<AdminDataEnvelope<Source[]>>('/admin/sources', { signal })).data;
  }

  async createSource(input: SourceCreateInput, signal?: AbortSignal): Promise<AdminMutationEnvelope<Source>> {
    return this.transport.json('/admin/sources', jsonInit('POST', input, signal));
  }

  async updateSource(id: string, input: SourceWriteInput, signal?: AbortSignal): Promise<AdminMutationEnvelope<Source>> {
    return this.transport.json(`/admin/sources/${encodePath(id)}`, jsonInit('PUT', input, signal));
  }

  async setSourceEnabled(id: string, enabled: boolean, signal?: AbortSignal): Promise<AdminMutationEnvelope<Source>> {
    return this.transport.json(
      `/admin/sources/${encodePath(id)}/enabled`,
      jsonInit('PUT', { enabled }, signal),
    );
  }

  async deleteSource(id: string, signal?: AbortSignal): Promise<void> {
    await this.transport.json<void>(`/admin/sources/${encodePath(id)}`, jsonInit('DELETE', undefined, signal));
  }

  async sourcePresetDiff(id: string, signal?: AbortSignal): Promise<ProviderPresetDiff> {
    return (await this.transport.json<AdminDataEnvelope<ProviderPresetDiff>>(
      `/admin/sources/${encodePath(id)}/preset-diff`,
      { signal },
    )).data;
  }

  async testConnection(
    sourceId: string,
    input: ConnectionTestInput,
    signal?: AbortSignal,
  ): Promise<ConnectionTestResult> {
    return (await this.transport.json<AdminDataEnvelope<ConnectionTestResult>>(
      `/admin/sources/${encodePath(sourceId)}/connection-tests`,
      jsonInit('POST', input, signal),
    )).data;
  }

  async accounts(signal?: AbortSignal): Promise<Account[]> {
    return (await this.transport.json<AdminDataEnvelope<Account[]>>('/admin/accounts', { signal })).data;
  }

  async createAccount(input: AccountWriteInput, signal?: AbortSignal): Promise<AdminMutationEnvelope<Account>> {
    return this.transport.json('/admin/accounts', jsonInit('POST', input, signal));
  }

  async updateAccount(id: string, input: AccountWriteInput, signal?: AbortSignal): Promise<AdminMutationEnvelope<Account>> {
    return this.transport.json(`/admin/accounts/${encodePath(id)}`, jsonInit('PUT', input, signal));
  }

  async setAccountEnabled(id: string, enabled: boolean, signal?: AbortSignal): Promise<AdminMutationEnvelope<Account>> {
    return this.transport.json(
      `/admin/accounts/${encodePath(id)}/enabled`,
      jsonInit('PUT', { enabled }, signal),
    );
  }

  async deleteAccount(id: string, signal?: AbortSignal): Promise<void> {
    await this.transport.json<void>(`/admin/accounts/${encodePath(id)}`, jsonInit('DELETE', undefined, signal));
  }

  async runDiscovery(
    sourceId: string,
    accountId: string,
    signal?: AbortSignal,
  ): Promise<DiscoveryExecution> {
    return (await this.transport.json<AdminDataEnvelope<DiscoveryExecution>>(
      `/admin/sources/${encodePath(sourceId)}/discoveries`,
      jsonInit('POST', { account_id: accountId, requested_by: 'admin-ui' }, signal),
    )).data;
  }

  async latestDiscovery(sourceId: string, signal?: AbortSignal): Promise<LatestDiscovery | null> {
    try {
      const response = await this.transport.json<{
        data: LatestDiscovery['run'];
        diff: LatestDiscovery['diff'];
        last_discovered_at: string;
      }>(`/admin/sources/${encodePath(sourceId)}/discoveries/latest`, { signal });
      return { run: response.data, diff: response.diff, last_discovered_at: response.last_discovered_at };
    } catch (error) {
      const normalized = normalizeAdminError(error);
      if (normalized.status === 404 && normalized.code === 'discovery_not_found') return null;
      throw error;
    }
  }

  async sourceModels(
    sourceId: string,
    filters: SourceModelFilters = {},
    signal?: AbortSignal,
  ): Promise<SourceModel[]> {
    const search = new URLSearchParams();
    if (filters.confirmationStatus) search.set('confirmation_status', filters.confirmationStatus);
    if (filters.availabilityStatus) search.set('availability_status', filters.availabilityStatus);
    const query = search.toString();
    return (await this.transport.json<AdminDataEnvelope<SourceModel[]>>(
      `/admin/sources/${encodePath(sourceId)}/models${query ? `?${query}` : ''}`,
      { signal },
    )).data;
  }

  async editSourceModel(
    sourceId: string,
    input: SourceModelEditInput,
    signal?: AbortSignal,
  ): Promise<SourceModel> {
    return (await this.transport.json<AdminDataEnvelope<SourceModel>>(
      `/admin/sources/${encodePath(sourceId)}/models`,
      jsonInit('PATCH', input, signal),
    )).data;
  }

  async confirmSourceModels(
    sourceId: string,
    models: SourceModelConfirmation[],
    signal?: AbortSignal,
  ): Promise<SourceModel[]> {
    return (await this.transport.json<AdminDataEnvelope<SourceModel[]>>(
      `/admin/sources/${encodePath(sourceId)}/models/confirm`,
      jsonInit('POST', { models }, signal),
    )).data;
  }

  async sourceModelCapabilities(
    sourceId: string,
    upstreamModelId: string,
    signal?: AbortSignal,
  ): Promise<SourceModelCapability[]> {
    return (await this.transport.json<AdminDataEnvelope<SourceModelCapability[]>>(
      `/admin/sources/${encodePath(sourceId)}/models/${encodePath(upstreamModelId)}/capabilities`,
      { signal },
    )).data;
  }

  async upsertSourceModelCapability(
    sourceId: string,
    upstreamModelId: string,
    protocol: GatewayProtocol,
    input: SourceModelCapabilityWrite,
    signal?: AbortSignal,
  ): Promise<AdminMutationEnvelope<SourceModelCapability>> {
    return this.transport.json(
      `/admin/sources/${encodePath(sourceId)}/models/${encodePath(upstreamModelId)}/capabilities/${encodePath(protocol)}`,
      jsonInit('PUT', input, signal),
    );
  }

  async logicalModels(signal?: AbortSignal): Promise<LogicalModel[]> {
    return (await this.transport.json<AdminDataEnvelope<LogicalModel[]>>(
      '/admin/logical-models',
      { signal },
    )).data;
  }

  async createLogicalModel(
    input: LogicalModelWriteInput,
    signal?: AbortSignal,
  ): Promise<AdminMutationEnvelope<LogicalModel>> {
    return this.transport.json('/admin/logical-models', jsonInit('POST', input, signal));
  }

  async updateLogicalModel(
    id: string,
    input: LogicalModelWriteInput,
    signal?: AbortSignal,
  ): Promise<AdminMutationEnvelope<LogicalModel>> {
    return this.transport.json(`/admin/logical-models/${encodePath(id)}`, jsonInit('PUT', input, signal));
  }

  async setLogicalModelEnabled(
    id: string,
    enabled: boolean,
    signal?: AbortSignal,
  ): Promise<AdminMutationEnvelope<LogicalModel>> {
    return this.transport.json(
      `/admin/logical-models/${encodePath(id)}/enabled`,
      jsonInit('PUT', { enabled }, signal),
    );
  }

  async deleteLogicalModel(id: string, signal?: AbortSignal): Promise<void> {
    await this.transport.json<void>(`/admin/logical-models/${encodePath(id)}`, jsonInit('DELETE', undefined, signal));
  }

  async modelBindings(signal?: AbortSignal): Promise<ModelBinding[]> {
    return (await this.transport.json<AdminDataEnvelope<ModelBinding[]>>(
      '/admin/model-bindings',
      { signal },
    )).data;
  }

  async createModelBinding(
    input: ModelBindingWriteInput,
    signal?: AbortSignal,
  ): Promise<AdminMutationEnvelope<ModelBinding>> {
    return this.transport.json('/admin/model-bindings', jsonInit('POST', input, signal));
  }

  async updateModelBinding(
    id: number,
    input: ModelBindingWriteInput,
    signal?: AbortSignal,
  ): Promise<AdminMutationEnvelope<ModelBinding>> {
    return this.transport.json(`/admin/model-bindings/${encodePath(id)}`, jsonInit('PUT', input, signal));
  }

  async setModelBindingEnabled(
    id: number,
    enabled: boolean,
    signal?: AbortSignal,
  ): Promise<AdminMutationEnvelope<ModelBinding>> {
    return this.transport.json(
      `/admin/model-bindings/${encodePath(id)}/enabled`,
      jsonInit('PUT', { enabled }, signal),
    );
  }

  async deleteModelBinding(id: number, signal?: AbortSignal): Promise<void> {
    await this.transport.json<void>(`/admin/model-bindings/${encodePath(id)}`, jsonInit('DELETE', undefined, signal));
  }

  async routes(signal?: AbortSignal): Promise<Route[]> {
    return (await this.transport.json<AdminDataEnvelope<Route[]>>('/admin/routes', { signal })).data;
  }

  async createRoute(input: RouteWriteInput, signal?: AbortSignal): Promise<AdminMutationEnvelope<Route>> {
    return this.transport.json('/admin/routes', jsonInit('POST', input, signal));
  }

  async updateRoute(
    id: string,
    input: RouteWriteInput,
    signal?: AbortSignal,
  ): Promise<AdminMutationEnvelope<Route>> {
    return this.transport.json(`/admin/routes/${encodePath(id)}`, jsonInit('PUT', input, signal));
  }

  async setRouteEnabled(id: string, enabled: boolean, signal?: AbortSignal): Promise<AdminMutationEnvelope<Route>> {
    return this.transport.json(
      `/admin/routes/${encodePath(id)}/enabled`,
      jsonInit('PUT', { enabled }, signal),
    );
  }

  async deleteRoute(id: string, signal?: AbortSignal): Promise<void> {
    await this.transport.json<void>(`/admin/routes/${encodePath(id)}`, jsonInit('DELETE', undefined, signal));
  }

  capabilities(signal?: AbortSignal): Promise<CapabilityMatrixResponse> {
    return this.transport.json('/admin/capabilities', { signal });
  }

  runtimeEvents(filters: RuntimeEventFilters = {}, signal?: AbortSignal): Promise<RuntimeEventResponse> {
    const search = new URLSearchParams();
    for (const [name, value] of Object.entries(filters)) {
      if (value !== undefined && value !== null && value !== '') search.set(name, String(value));
    }
    const query = search.toString();
    return this.transport.json(`/admin/events${query ? `?${query}` : ''}`, { signal });
  }

  eventTypeOptions(range: FilterOptionsRange, search: string, signal?: AbortSignal): Promise<FilterOptionsResponse> {
    const params = new URLSearchParams({ field: 'event_type', q: search });
    if (range.from) params.set('from', range.from);
    if (range.to) params.set('to', range.to);
    return this.transport.json(`/admin/events/filter-options?${params}`, { signal });
  }

  async virtualKeys(signal?: AbortSignal): Promise<VirtualKey[]> {
    return (await this.transport.json<AdminDataEnvelope<VirtualKey[]>>('/admin/keys', { signal })).data;
  }

  createVirtualKey(input: VirtualKeyCreateInput, signal?: AbortSignal): Promise<VirtualKeyCreateResult> {
    return this.transport.json('/admin/keys', jsonInit('POST', input, signal));
  }

  async revealVirtualKey(id: number, signal?: AbortSignal): Promise<VirtualKeySecret> {
    return (await this.transport.json<AdminDataEnvelope<VirtualKeySecret>>(
      `/admin/keys/${encodePath(id)}/value`,
      { signal },
    )).data;
  }

  async revokeVirtualKey(id: number, signal?: AbortSignal): Promise<void> {
    await this.transport.json(`/admin/keys/${encodePath(id)}/revoke`, jsonInit('POST', {}, signal));
  }

  rotateVirtualKey(id: number, input: VirtualKeyRotateInput, signal?: AbortSignal): Promise<VirtualKeyRotateResult> {
    return this.transport.json(`/admin/keys/${encodePath(id)}/rotate`, jsonInit('POST', input, signal));
  }

  reloadRuntime(signal?: AbortSignal): Promise<RuntimeReloadResult> {
    return this.transport.json('/admin/config/reload', jsonInit('POST', {}, signal));
  }

  async sanitizedConfigurationExport(signal?: AbortSignal): Promise<SanitizedConfigurationExport> {
    const [sources, accounts, logicalModels, modelBindings, routes, capabilities] = await Promise.all([
      this.sources(signal),
      this.accounts(signal),
      this.logicalModels(signal),
      this.modelBindings(signal),
      this.routes(signal),
      this.capabilities(signal),
    ]);
    return {
      version: 'v1',
      exported_at: new Date().toISOString(),
      fact_source: 'admin_resources',
      snapshot: {
        revision: capabilities.snapshot_revision,
        generated_at: capabilities.snapshot_generated_at,
      },
      sources,
      accounts: accounts.map(({ credential_env: _credentialEnv, ...account }) => account),
      logical_models: logicalModels,
      model_bindings: modelBindings,
      routes,
    };
  }
}
