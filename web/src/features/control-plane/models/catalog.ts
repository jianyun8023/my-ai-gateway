import type {
  Account,
  CapabilityMatrixResponse,
  CatalogStatus,
  EffectiveProtocolCapability,
  LogicalModel,
  ModelBinding,
  Route,
  Source
} from '@/admin-api';

export interface CatalogData {
  logicalModels: LogicalModel[];
  bindings: ModelBinding[];
  routes: Route[];
  sources: Source[];
  accounts: Account[];
  capabilities: CapabilityMatrixResponse;
}

export type Editor =
  | { kind: 'logical-model'; record?: LogicalModel }
  | { kind: 'binding'; record?: ModelBinding }
  | { kind: 'route'; record?: Route };

export type DeleteTarget =
  | { kind: 'logical-model'; record: LogicalModel }
  | { kind: 'binding'; record: ModelBinding }
  | { kind: 'route'; record: Route };

export type DetailTarget = DeleteTarget;

export const statusTone = (status: CatalogStatus) => {
  if (status === 'confirmed') return 'success' as const;
  if (status === 'unavailable') return 'danger' as const;
  return 'warning' as const;
};

export const statusOptions = (record?: { status: CatalogStatus }): CatalogStatus[] => {
  if (!record || record.status === 'pending') return ['pending', 'confirmed', 'unavailable'];
  if (record.status === 'confirmed') return ['confirmed', 'unavailable'];
  return ['unavailable', 'pending'];
};

export interface ResolvedBindingCell {
  routeId: string;
  model: string;
  upstreamModel: string;
  cell: EffectiveProtocolCapability;
}

export const resolvedCellsForBinding = (
  capabilities: CapabilityMatrixResponse,
  bindingId: number,
): ResolvedBindingCell[] => capabilities.data.flatMap((row) => row.protocols
  .filter((cell) => cell.binding_id === bindingId && cell.status === 'routable')
  .map((cell) => ({ routeId: row.route_id, model: row.model, upstreamModel: row.upstream_model_id, cell })));
