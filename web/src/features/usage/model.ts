import { type UsageBreakdownDimension } from '@/gateway-usage';

export type TrendMetric = 'composition' | 'total' | 'input' | 'output' | 'reasoning' | 'cached' | 'requests';

export const ANALYSIS_DIMENSIONS: Array<{ dimension: UsageBreakdownDimension; titleKey: string }> = [
  { dimension: 'logical_model', titleKey: 'usage.field.logical_model' },
  { dimension: 'upstream_model', titleKey: 'usage.field.upstream_model' },
  { dimension: 'provider', titleKey: 'usage.field.provider' },
  { dimension: 'source_id', titleKey: 'usage.field.source_id' },
  { dimension: 'client_source', titleKey: 'usage.field.client_source' },
  { dimension: 'account', titleKey: 'usage.field.account' },
  { dimension: 'protocol_in', titleKey: 'usage.field.protocol_in' },
  { dimension: 'protocol_upstream', titleKey: 'usage.field.protocol_upstream' },
];
