import type { EffectiveProtocolCapability, GatewayProtocol, LogicalModel } from '@/admin-api';
import type { CatalogData } from './catalog';
import { summarizeModelRouting } from './routingPresentation';

/** Scope runtime cells to the routes that actually handle the selected protocol. */
export function modelProtocolCapabilities(model: LogicalModel, data: CatalogData, protocol: GatewayProtocol) {
  const summary = summarizeModelRouting(model, data);
  const routes = data.routes.filter((route) => route.logical_model_id === model.id && route.protocols.includes(protocol));
  const rows = data.capabilities.data.filter((row) => row.model === model.public_name);
  const path = summary.paths.find((item) => item.protocols.includes(protocol));
  const entries = rows.flatMap((row) => {
    const route = routes.find((item) => item.id === row.route_id);
    if (!route) return [];
    const cell = row.protocols.find((item) => item.protocol_in === protocol);
    const line = path?.entries.find((item) => item.line.sourceId === row.source.source_id
      && item.line.accountId === row.account.account_id && item.line.upstreamModelId === row.upstream_model_id);
    // The routing summary already reconciles model/route/binding state with the
    // snapshot. Do not republish an older routable cell that it no longer selects.
    if (cell?.status === 'routable' && (!line || line.role === 'unselected')) return [];
    return [{ row, route, cell, line }];
  }).sort((a, b) => (a.cell?.selection_rank ?? Number.MAX_SAFE_INTEGER) - (b.cell?.selection_rank ?? Number.MAX_SAFE_INTEGER));
  const unpublished = (path?.entries ?? []).filter(({ line }) => !entries.some(({ row }) =>
    row.source.source_id === line.sourceId && row.account.account_id === line.accountId && row.upstream_model_id === line.upstreamModelId));
  const configured = routes.length > 0 || unpublished.length > 0;

  // Failed protocols may have no row of their own: the API copies the resolver's
  // model/protocol error into the other rows. Preserve those errors once without
  // assigning them to an unrelated source/account. Off-route binding placeholders
  // only mean that this row does not handle the protocol.
  const errors = new Map<string, NonNullable<EffectiveProtocolCapability['error']>>();
  if (configured && !entries.some(({ cell }) => cell?.status === 'routable')) {
    for (const row of rows) {
      const cell = row.protocols.find((item) => item.protocol_in === protocol);
      if (cell?.status === 'unroutable' && cell.error && cell.error.code !== 'runtime_binding_not_available'
        && !entries.some((entry) => entry.cell?.error?.code === cell.error?.code && entry.cell?.error?.message === cell.error?.message)) {
        errors.set(JSON.stringify(cell.error), cell.error);
      }
    }
  }
  return { entries, unpublished, configured, errors: [...errors.values()], summary };
}
