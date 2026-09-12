import type {
  Account,
  CapabilityMatrixResponse,
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
