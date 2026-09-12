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

const chat: GatewayProtocol = 'openai_chat_completions';
const responses: GatewayProtocol = 'openai_responses';
const messages: GatewayProtocol = 'anthropic_messages';
const timestamp = '2026-09-12T00:00:00Z';
const model: LogicalModel = {
  id: 'model-a', public_name: 'public-model', display_name: 'Model A', status: 'confirmed',
  metadata: {}, field_sources: {}, enabled: true, created_at: timestamp, updated_at: timestamp,
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
