import type {
  Account,
  CapabilityMatrixRow,
  EffectiveProtocolCapability,
  GatewayProtocol,
  LogicalModel,
  ModelBinding,
  Route,
  Source,
} from '@/admin-api';
import { describe, expect, it, vi, afterEach } from 'vitest';
import type { CatalogData } from './catalog';
import { summarizeModelRouting } from './routingPresentation';
import { modelProtocolCapabilities } from './modelCapabilities';

const chat: GatewayProtocol = 'openai_chat_completions';
const responses: GatewayProtocol = 'openai_responses';
const messages: GatewayProtocol = 'anthropic_messages';
const timestamp = '2026-09-12T00:00:00Z';
const model: LogicalModel = {
  id: 'model-a', public_name: 'public-model', display_name: 'Model A', status: 'confirmed',
  metadata: {}, field_sources: {}, enabled: true, request_timeout_ms: null, max_retries: null, created_at: timestamp, updated_at: timestamp,
};

function binding(id: number, line: string, protocol: GatewayProtocol = chat): ModelBinding {
  return {
    id, logical_model_id: model.id, source_id: `source-${line}`, account_id: `account-${line}`,
    upstream_model_id: `upstream-${line}`, protocol, status: 'confirmed', enabled: true,
    priority: 100 - id, created_at: timestamp, updated_at: timestamp,
  };
}

function source(line: string): Source {
  return {
    id: `source-${line}`, display_name: `Source ${line}`, provider_preset_id: 'preset',
    provider_preset_version: 1, provider_preset_snapshot: {}, base_url: 'https://example.com',
    endpoints: {}, auth_config: {}, protocol_capabilities: {}, enabled: true,
    created_at: timestamp, updated_at: timestamp,
  };
}

function account(line: string): Account {
  return {
    id: `account-${line}`, source_id: `source-${line}`, display_name: `Account ${line}`,
    credential_configured: true, enabled: true, weight: 100, health_status: 'healthy',
    created_at: timestamp, updated_at: timestamp,
  };
}

function route(protocols: GatewayProtocol[] = [chat], strategy = 'ordered_fallback'): Route {
  return {
    id: 'route-a', logical_model_id: model.id, public_name: model.public_name, protocols, strategy,
    allow_lossy_conversion: false, enabled: true, created_at: timestamp, updated_at: timestamp,
  };
}

function capability(item: ModelBinding, rank: number, overrides: Partial<EffectiveProtocolCapability> = {}): EffectiveProtocolCapability {
  return {
    protocol_in: item.protocol, status: 'routable', binding_id: item.id,
    selection: rank === 0 ? 'primary' : 'fallback', selection_rank: rank,
    protocol_upstream: item.protocol, endpoint: 'https://example.com/v1/chat', mode: 'native',
    conversion_chain: [],
    effective_capabilities: {
      streaming: 'native', tools: 'native', tool_streaming: 'native', thinking: 'unsupported',
      web_search: 'unsupported', file_search: 'unsupported', vision: 'unsupported', usage: 'native',
    },
    degraded: false, degraded_features: [], ...overrides,
  };
}

function row(item: ModelBinding, cells: EffectiveProtocolCapability[]): CapabilityMatrixRow {
  return {
    route_id: 'route-a', source: { source_id: item.source_id }, account: { account_id: item.account_id, enabled: true },
    model: model.public_name, model_display_name: model.display_name, upstream_model_id: item.upstream_model_id,
    protocols: cells,
  };
}

function catalog(bindings: ModelBinding[], rows: CapabilityMatrixRow[], routes = [route()]): CatalogData {
  return {
    logicalModels: [model], bindings, routes,
    sources: ['a', 'b', 'c'].map(source), accounts: ['a', 'b', 'c'].map(account),
    capabilities: {
      version: 'v1', fact_source: 'runtime_snapshot', snapshot_revision: 1,
      snapshot_generated_at: timestamp, data: rows,
    },
  };
}

afterEach(() => vi.useRealTimers());

describe('model-level routing presentation', () => {
  it.each([0, 1, null])('keeps all candidate lines while exposing the actual retry cap %s', (maxRetries) => {
    const bindings = [binding(1, 'a'), binding(2, 'b'), binding(3, 'c')];
    const data = catalog(bindings, bindings.map((item, index) => row(item, [capability(item, index)])));
    data.accounts[0].health_status = 'cooling_down';
    data.accounts[0].cooldown_until = '2099-01-01T00:00:00Z';
    const summary = summarizeModelRouting({ ...model, max_retries: maxRetries }, data);
    expect(summary.paths[0].maxAttempts).toBe(maxRetries === null ? null : maxRetries + 1);
    // A cooling primary is skipped without consuming the cap; later lines must remain visible.
    expect(summary.paths[0].entries.map((entry) => entry.line.accountId)).toEqual(['account-a', 'account-b', 'account-c']);
    data.routes[0].strategy = 'primary_then_weighted_fallback';
    expect(summarizeModelRouting({ ...model, max_retries: maxRetries }, data).paths[0].maxAttempts).toBeNull();
  });

  it('merges protocol bindings into two actual lines and one shared plan', () => {
    const aChat = binding(1, 'a');
    const aResponses = binding(2, 'a', responses);
    const bChat = binding(3, 'b');
    const bResponses = binding(4, 'b', responses);
    const data = catalog([aChat, aResponses, bChat, bResponses], [
      row(aChat, [capability(aChat, 0), capability(aResponses, 0)]),
      row(bChat, [capability(bChat, 1), capability(bResponses, 1)]),
    ], [route([chat, responses])]);

    const summary = summarizeModelRouting(model, data);
    expect(summary.lines).toHaveLength(2);
    expect(summary.paths).toHaveLength(1);
    expect(summary.paths[0].protocols).toEqual([chat, responses]);
    expect(summary.paths[0].entries.map((entry) => [entry.line.accountId, entry.role, entry.backupIndex])).toEqual([
      ['account-a', 'primary', undefined], ['account-b', 'backup', 1],
    ]);
    expect(summary.protocolSpecific).toBe(false);
    expect(summary.status).toBe('healthy');
    expect(summary.protocols.find((item) => item.protocol === messages)).toMatchObject({ supported: false, mode: 'unknown' });
  });

  it('keeps reversed protocol primaries in separate plans inside the same model', () => {
    const aChat = binding(1, 'a');
    const aResponses = binding(2, 'a', responses);
    const bChat = binding(3, 'b');
    const bResponses = binding(4, 'b', responses);
    const data = catalog([aChat, aResponses, bChat, bResponses], [
      row(aChat, [capability(aChat, 0), capability(aResponses, 1)]),
      row(bChat, [capability(bChat, 1), capability(bResponses, 0)]),
    ], [route([chat, responses])]);

    const summary = summarizeModelRouting(model, data);
    expect(summary.lines).toHaveLength(2);
    expect(summary.protocolSpecific).toBe(true);
    expect(summary.paths.map((path) => path.entries.map((entry) => entry.line.accountId))).toEqual([
      ['account-a', 'account-b'], ['account-b', 'account-a'],
    ]);
  });

  it('preserves a weighted backup pool without promising an order from selection ranks', () => {
    const bindings = [binding(1, 'a'), binding(2, 'b'), binding(3, 'c')];
    const data = catalog(bindings, bindings.map((item, index) => row(item, [capability(item, index)])), [route([chat], 'primary_then_weighted_fallback')]);

    const path = summarizeModelRouting(model, data).paths[0];
    expect(path.strategy).toBe('weighted');
    expect(path.entries.map((entry) => entry.role)).toEqual(['primary', 'backup', 'backup']);
    expect(path.entries.map((entry) => entry.backupIndex)).toEqual([undefined, undefined, undefined]);
  });

  it('retains native support when account health is unknown', () => {
    const item = binding(1, 'a');
    const data = catalog([item], [row(item, [capability(item, 0)])]);
    data.accounts[0].health_status = 'unknown';

    const summary = summarizeModelRouting(model, data);
    expect(summary.protocols[0]).toMatchObject({ mode: 'native', supported: true, status: 'unknown' });
    expect(summary.status).toBe('unknown');
    expect(summary.lines[0].healthStatus).toBe('unknown');
  });

  it('shows cooling and disabled lines even when the runtime plan still references them', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(timestamp));
    const bindings = [binding(1, 'a'), binding(2, 'b'), binding(3, 'c')];
    const data = catalog(bindings, bindings.map((item, index) => row(item, [capability(item, index)])));
    data.accounts[0].cooldown_until = '2026-09-12T00:30:00Z';
    data.accounts[0].health_status = 'cooling_down';
    data.accounts[1].enabled = false;

    const summary = summarizeModelRouting(model, data);
    expect(summary.lines.map((line) => [line.healthStatus, line.status])).toEqual([
      ['cooling_down', 'unavailable'], ['disabled', 'disabled'], ['healthy', 'healthy'],
    ]);
    expect(summary.status).toBe('degraded');
    expect(summary.protocols[0].supported).toBe(true);
  });

  it('reports native and adapter paths together and retains explicit degradation', () => {
    const primary = binding(1, 'a');
    const backup = binding(2, 'b');
    const data = catalog([primary, backup], [
      row(primary, [capability(primary, 0)]),
      row(backup, [capability(backup, 1, { mode: 'adapter', protocol_upstream: messages, adapter: 'test-adapter', degraded: true, degraded_features: ['thinking'] })]),
    ]);

    const summary = summarizeModelRouting(model, data);
    expect(summary.protocols[0]).toMatchObject({ mode: 'mixed', supported: true, degraded: true, status: 'degraded' });
    expect(summary.status).toBe('degraded');
  });

  it('does not promote source declarations or unpublished bindings to model support', () => {
    const item = binding(1, 'a');
    const data = catalog([item], []);
    data.sources[0].protocol_capabilities = { [chat]: { mode: 'native' } };

    const summary = summarizeModelRouting(model, data);
    expect(summary.protocols[0]).toMatchObject({ mode: 'unknown', supported: false, status: 'unavailable' });
    expect(summary.paths[0].entries[0]).toMatchObject({ role: 'unselected', status: 'unavailable' });
    expect(summary.status).toBe('unavailable');
  });

  it('distinguishes an explicit unsupported error from a missing model capability', () => {
    const item = binding(1, 'a');
    const data = catalog([item], [row(item, [capability(item, 0, {
      status: 'unroutable', binding_id: null, mode: null, selection: null, selection_rank: null,
      error: { code: 'unsupported_protocol', message: 'Unsupported protocol' },
    })])]);

    expect(summarizeModelRouting(model, data).protocols[0]).toMatchObject({ mode: 'unsupported', supported: false });
  });

  it('keeps an unknown runtime mode separate from a healthy account', () => {
    const item = binding(1, 'a');
    const data = catalog([item], [row(item, [capability(item, 0, { mode: null })])]);
    const summary = summarizeModelRouting(model, data);
    expect(summary.protocols[0]).toMatchObject({ mode: 'unknown', supported: false, status: 'unknown' });
    expect(summary.lines[0].healthStatus).toBe('healthy');
    expect(summary.status).toBe('unknown');
  });

  it.each([
    [{ enabled: false }, 'disabled'],
    [{ status: 'pending' }, 'pending'],
    [{ status: 'unavailable' }, 'unavailable'],
  ] as const)('prioritizes model state %o over an older published plan', (change, status) => {
    const item = binding(1, 'a');
    const data = catalog([item], [row(item, [capability(item, 0)])]);
    const summary = summarizeModelRouting({ ...model, ...change }, data);
    expect(summary.status).toBe(status);
    expect(summary.protocols[0]).toMatchObject({ status, supported: false });
    expect(summary.paths[0].entries[0].role).toBe('unselected');
  });

  it('retains an unpublished pending binding without treating it as a backup', () => {
    const primary = binding(1, 'a');
    const pending = { ...binding(2, 'b'), status: 'pending' as const };
    const data = catalog([primary, pending], [row(primary, [capability(primary, 0)])]);

    const summary = summarizeModelRouting(model, data);
    expect(summary.paths[0].entries[1]).toMatchObject({ role: 'unselected', status: 'pending' });
    expect(summary.status).toBe('degraded');
  });
});

describe('model capability detail scope', () => {
  const missing = (item: ModelBinding, protocol: GatewayProtocol, code = 'runtime_binding_not_available') => capability(item, 0, {
    protocol_in: protocol, status: 'unroutable', binding_id: null, mode: null, selection: null,
    error: { code, message: `No available binding for ${protocol}`, route_id: 'route-a' },
  });

  it('shows one actual line per protocol when three separate routes contain cross-protocol placeholders', () => {
    const protocols = [chat, responses, messages];
    const bindings = protocols.map((protocol, index) => binding(index + 1, 'a', protocol));
    const routes = protocols.map((protocol) => ({ ...route([protocol]), id: `route-${protocol}` }));
    const rows = bindings.map((item, index) => ({
      ...row(item, protocols.map((protocol) => protocol === item.protocol ? capability(item, 0) : missing(item, protocol))),
      route_id: routes[index].id,
    }));
    const data = catalog(bindings, rows, routes);
    for (const protocol of protocols) {
      const detail = modelProtocolCapabilities(model, data, protocol);
      expect(detail.entries).toHaveLength(1);
      expect(detail.entries[0].cell).toMatchObject({ protocol_in: protocol, status: 'routable' });
      expect(detail.errors).toEqual([]);
      expect(detail.unpublished).toEqual([]);
    }
  });

  it('retains an actual unavailable cell on a route that handles the selected protocol', () => {
    const item = binding(1, 'a');
    const errorCell = missing(item, chat);
    const detail = modelProtocolCapabilities(model, catalog([item], [row(item, [errorCell])]), chat);
    expect(detail.entries[0].cell?.error?.code).toBe('runtime_binding_not_available');
    expect(detail.entries[0].cell?.status).toBe('unroutable');
  });

  it('preserves a model/protocol resolver error carried by other rows without claiming their account as the failed line', () => {
    const a = binding(1, 'a');
    const b = binding(2, 'b');
    const error = missing(a, responses, 'unsupported_protocol');
    const data = catalog([a, b], [row(a, [capability(a, 0), error]), row(b, [capability(b, 1), error])], [
      route(), { ...route([responses]), id: 'route-responses' },
    ]);
    const detail = modelProtocolCapabilities(model, data, responses);
    expect(detail.entries).toEqual([]);
    expect(detail.errors).toEqual([error.error]);
    expect(detail.configured).toBe(true);
  });

  it('keeps missing snapshot data distinct from an unconfigured protocol', () => {
    const item = binding(1, 'a');
    const data = catalog([item], []);
    const detail = modelProtocolCapabilities(model, data, chat);
    expect(detail.configured).toBe(true);
    expect(detail.entries).toEqual([]);
    expect(detail.unpublished).toHaveLength(1);
    expect(detail.unpublished[0].status).toBe('unavailable');
    expect(modelProtocolCapabilities(model, data, responses)).toMatchObject({ configured: false, entries: [], unpublished: [], errors: [] });
  });

  it('does not replace a missing protocol cell with another protocol or preset capabilities', () => {
    const item = binding(1, 'a');
    const data = catalog([item], [row(item, [capability(item, 0, { protocol_in: responses })])]);
    const detail = modelProtocolCapabilities(model, data, chat);
    expect(detail.entries).toHaveLength(1);
    expect(detail.entries[0].cell).toBeUndefined();
  });

  it('retains disabled, pending and cooling lines omitted from the published matrix', () => {
    const a = { ...binding(1, 'a'), enabled: false };
    const b = { ...binding(2, 'b'), status: 'pending' as const };
    const c = binding(3, 'c');
    const data = catalog([a, b, c], []);
    data.accounts[2].health_status = 'cooling_down';
    data.accounts[2].cooldown_until = '2099-01-01T00:00:00Z';
    const detail = modelProtocolCapabilities(model, data, chat);
    expect(detail.unpublished.map((entry) => entry.status)).toEqual(['disabled', 'pending', 'unavailable']);
  });

  it('keeps protocol-specific primary order and degraded feature facts', () => {
    const a = binding(1, 'a');
    const aResponses = binding(2, 'a', responses);
    const b = binding(3, 'b');
    const bResponses = binding(4, 'b', responses);
    const degraded = capability(aResponses, 1, { degraded: true, degraded_features: ['thinking'], mode: 'adapter' });
    const data = catalog([a, aResponses, b, bResponses], [
      row(a, [capability(a, 0), degraded]), row(b, [capability(b, 1), capability(bResponses, 0)]),
    ], [route([chat, responses])]);
    expect(modelProtocolCapabilities(model, data, chat).entries.map(({ row }) => row.account.account_id)).toEqual(['account-a', 'account-b']);
    const detail = modelProtocolCapabilities(model, data, responses);
    expect(detail.entries.map(({ row }) => row.account.account_id)).toEqual(['account-b', 'account-a']);
    expect(detail.entries[1].cell).toEqual(degraded);
  });
});
