import {
  GATEWAY_PROTOCOLS,
  type Account,
  type EffectiveProtocolCapability,
  type GatewayProtocol,
  type LogicalModel,
  type ModelBinding,
  type Route,
  type Source,
} from '@/admin-api';
import type { CatalogData } from './catalog';

type ModelRoutingStatus = 'healthy' | 'degraded' | 'unavailable' | 'disabled' | 'pending' | 'unknown';
type ModelProtocolMode = 'native' | 'adapter' | 'mixed' | 'unsupported' | 'unknown';

interface ModelRoutingProtocol {
  protocol: GatewayProtocol;
  mode: ModelProtocolMode;
  status: ModelRoutingStatus;
  /** Protocol support and account health are independent facts. */
  supported: boolean;
  degraded: boolean;
}

interface ModelRouteLine {
  id: string;
  sourceId: string;
  sourceName: string;
  accountId: string;
  accountName: string;
  upstreamModelId: string;
  bindings: ModelBinding[];
  protocols: GatewayProtocol[];
  status: ModelRoutingStatus;
  healthStatus: string;
  cooldownUntil?: string | null;
}

export interface ModelRoutePathEntry {
  line: ModelRouteLine;
  role: 'primary' | 'backup' | 'unselected';
  /** Only deterministic ordered fallbacks receive a backup index. */
  backupIndex?: number;
  status: ModelRoutingStatus;
  mode: ModelProtocolMode;
  degraded: boolean;
}

export interface ModelRoutePathGroup {
  protocols: GatewayProtocol[];
  strategy: 'ordered' | 'weighted' | 'unknown';
  entries: ModelRoutePathEntry[];
}

export interface ModelRoutingSummary {
  protocols: ModelRoutingProtocol[];
  lines: ModelRouteLine[];
  paths: ModelRoutePathGroup[];
  status: ModelRoutingStatus;
  protocolSpecific: boolean;
}

interface RuntimeCell {
  route: Route;
  cell: EffectiveProtocolCapability;
  line: ModelRouteLine;
}

const lineId = (binding: ModelBinding) => JSON.stringify([
  binding.source_id, binding.account_id, binding.upstream_model_id,
]);

const modelState = (model: LogicalModel): ModelRoutingStatus | undefined => {
  if (!model.enabled) return 'disabled';
  if (model.status === 'pending') return 'pending';
  if (model.status === 'unavailable') return 'unavailable';
};

function accountHealth(account?: Account, source?: Source): string {
  if (!account || !source) return 'unknown';
  if (!account.enabled || !source.enabled) return 'disabled';
  if (account.cooldown_until && Date.parse(account.cooldown_until) > Date.now()) return 'cooling_down';
  if (account.health_status === 'cooling_down' && account.cooldown_until
    && Date.parse(account.cooldown_until) <= Date.now()) return 'unhealthy';
  return account.health_status || 'unknown';
}

function configuredLineStatus(bindings: ModelBinding[], health: string): ModelRoutingStatus {
  if (health === 'disabled' || bindings.every((binding) => !binding.enabled)) return 'disabled';
  const enabled = bindings.filter((binding) => binding.enabled);
  if (!enabled.some((binding) => binding.status === 'confirmed')) {
    return enabled.some((binding) => binding.status === 'pending') ? 'pending' : 'unavailable';
  }
  if (health === 'cooling_down') return 'unavailable';
  if (health === 'healthy') return 'healthy';
  if (health === 'degraded' || health === 'unhealthy') return 'degraded';
  return 'unknown';
}

function aggregateStatus(statuses: ModelRoutingStatus[]): ModelRoutingStatus {
  if (statuses.length === 0) return 'unavailable';
  const usable = statuses.some((status) => status === 'healthy' || status === 'degraded' || status === 'unknown');
  if (!usable) {
    if (statuses.every((status) => status === 'disabled')) return 'disabled';
    if (statuses.some((status) => status === 'pending')) return 'pending';
    return 'unavailable';
  }
  if (statuses.some((status) => status === 'degraded' || status === 'unavailable' || status === 'disabled' || status === 'pending')) return 'degraded';
  if (statuses.includes('unknown')) return 'unknown';
  return 'healthy';
}

function protocolMode(cells: EffectiveProtocolCapability[]): ModelProtocolMode {
  const modes = new Set(cells.filter((cell) => cell.status === 'routable').flatMap((cell) => cell.mode ? [cell.mode] : []));
  if (modes.has('native') && modes.has('adapter')) return 'mixed';
  if (modes.has('native')) return 'native';
  if (modes.has('adapter')) return 'adapter';
  if (cells.length > 0 && cells.every((cell) => cell.error?.code === 'unsupported_protocol' || cell.error?.code === 'adapter_source_unsupported')) return 'unsupported';
  return 'unknown';
}

function routeStrategy(routes: Route[]): ModelRoutePathGroup['strategy'] {
  const strategies = new Set(routes.map((route) => route.strategy));
  if (strategies.size !== 1) return 'unknown';
  if (strategies.has('ordered_fallback')) return 'ordered';
  if (strategies.has('primary_then_weighted_fallback')) return 'weighted';
  return 'unknown';
}

function makeLines(model: LogicalModel, data: CatalogData): ModelRouteLine[] {
  const grouped = new Map<string, ModelBinding[]>();
  for (const binding of data.bindings.filter((item) => item.logical_model_id === model.id)) {
    const id = lineId(binding);
    grouped.set(id, [...(grouped.get(id) ?? []), binding]);
  }
  return Array.from(grouped, ([id, bindings]) => {
    const first = bindings[0];
    const source = data.sources.find((item) => item.id === first.source_id);
    const account = data.accounts.find((item) => item.id === first.account_id && item.source_id === first.source_id);
    const healthStatus = accountHealth(account, source);
    return {
      id,
      sourceId: first.source_id,
      sourceName: source?.display_name || first.source_id,
      accountId: first.account_id,
      accountName: account?.display_name || first.account_id,
      upstreamModelId: first.upstream_model_id,
      bindings,
      protocols: GATEWAY_PROTOCOLS.filter((protocol) => bindings.some((binding) => binding.protocol === protocol)),
      status: configuredLineStatus(bindings, healthStatus),
      healthStatus,
      cooldownUntil: account?.cooldown_until,
    };
  });
}

/** Build a model-level read model without turning protocol bindings into model rows. */
export function summarizeModelRouting(model: LogicalModel, data: CatalogData): ModelRoutingSummary {
  const lines = makeLines(model, data);
  const routes = data.routes.filter((route) => route.logical_model_id === model.id);
  const rows = data.capabilities.data.filter((row) => row.model === model.public_name);
  const override = modelState(model);
  const runtime: RuntimeCell[] = [];
  if (!override) {
    for (const row of rows) {
      const route = routes.find((item) => item.id === row.route_id && item.enabled);
      if (!route) continue;
      for (const cell of row.protocols) {
        if (cell.status !== 'routable' || !route.protocols.includes(cell.protocol_in)) continue;
        const line = lines.find((item) => item.sourceId === row.source.source_id
          && item.accountId === row.account.account_id && item.upstreamModelId === row.upstream_model_id
          && item.bindings.some((binding) => binding.id === cell.binding_id && binding.enabled && binding.status === 'confirmed'));
        if (line) runtime.push({ route, cell, line });
      }
    }
  }

  const paths: ModelRoutePathGroup[] = [];
  const protocols = GATEWAY_PROTOCOLS.map((protocol): ModelRoutingProtocol => {
    const selected = runtime.filter(({ cell }) => cell.protocol_in === protocol).sort((left, right) =>
      (left.cell.selection_rank ?? Number.MAX_SAFE_INTEGER) - (right.cell.selection_rank ?? Number.MAX_SAFE_INTEGER));
    const configuredRoutes = routes.filter((route) => route.protocols.includes(protocol));
    const strategy = routeStrategy(selected.length ? selected.map(({ route }) => route) : configuredRoutes.filter((route) => route.enabled));
    const entries: ModelRoutePathEntry[] = [];
    for (const { cell, line } of selected) {
      if (entries.some((entry) => entry.line.id === line.id)) continue;
      const role = cell.selection === 'primary' ? 'primary' : cell.selection === 'fallback' ? 'backup' : 'unselected';
      entries.push({
        line,
        role,
        backupIndex: strategy === 'ordered' && role === 'backup' ? cell.selection_rank ?? undefined : undefined,
        status: cell.degraded && line.status === 'healthy' ? 'degraded' : line.status,
        mode: cell.mode ?? 'unknown',
        degraded: cell.degraded,
      });
    }
    for (const line of lines.filter((item) => item.protocols.includes(protocol))) {
      if (entries.some((entry) => entry.line.id === line.id)) continue;
      const bindings = line.bindings.filter((binding) => binding.protocol === protocol);
      const configuredStatus = configuredLineStatus(bindings, line.healthStatus);
      entries.push({
        line,
        role: 'unselected',
        status: override ?? (configuredRoutes.length > 0 && configuredRoutes.every((route) => !route.enabled)
          ? 'disabled' : configuredStatus === 'healthy' ? 'unavailable' : configuredStatus),
        mode: 'unknown',
        degraded: false,
      });
    }

    if (entries.length > 0) {
      // Protocols collapse only when they really share the same plan, including
      // the primary, backup order, strategy, publication state and degradation.
      const signature = (path: Pick<ModelRoutePathGroup, 'strategy' | 'entries'>) => JSON.stringify([
        path.strategy,
        path.entries.map((entry) => [entry.line.id, entry.role, entry.backupIndex, entry.status, entry.degraded]),
      ]);
      const matching = paths.find((path) => signature(path) === signature({ strategy, entries }));
      if (matching) matching.protocols.push(protocol);
      else paths.push({ protocols: [protocol], strategy, entries });
    }

    const cells = selected.length ? selected.map(({ cell }) => cell) : rows.flatMap((row) => row.protocols.filter((cell) => cell.protocol_in === protocol && cell.status === 'unroutable'));
    const mode = protocolMode(cells);
    const supported = mode === 'native' || mode === 'adapter' || mode === 'mixed';
    return {
      protocol,
      mode,
      supported,
      status: override ?? (selected.length > 0
        ? supported ? aggregateStatus(entries.map((entry) => entry.status)) : 'unknown'
        : 'unavailable'),
      degraded: selected.some(({ cell }) => cell.degraded),
    };
  });
  const configuredProtocols = protocols.filter(({ protocol }) =>
    routes.some((route) => route.protocols.includes(protocol)) || lines.some((line) => line.protocols.includes(protocol)));
  const status = override ?? aggregateStatus(configuredProtocols.map((protocol) => protocol.status));
  return { protocols, lines, paths, status, protocolSpecific: paths.length > 1 };
}
