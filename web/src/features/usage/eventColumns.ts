import { readLocalPreference } from '@/lib/browserStorage';

export const COLUMNS_STORAGE_KEY = 'my-ai-gateway-usage-event-columns-v2';

export const EVENT_COLUMNS = [
  'time',
  'logicalModel',
  'upstreamModel',
  'provider',
  'sourceAccount',
  'clientSource',
  'protocol',
  'status',
  'retries',
  'latency',
  'tokens',
  'usageSource',
] as const;

export type EventColumn = typeof EVENT_COLUMNS[number];

export const EVENT_COLUMN_LABELS: Record<EventColumn, string> = {
  time: 'usage.field.time',
  logicalModel: 'usage.field.logical_model',
  upstreamModel: 'usage.field.upstream_model',
  provider: 'usage.field.provider',
  sourceAccount: 'usage.field.source_account',
  clientSource: 'usage.field.client_source',
  protocol: 'usage.field.protocol',
  status: 'usage.field.status',
  retries: 'usage.field.retries',
  latency: 'usage.field.latency',
  tokens: 'usage.field.tokens',
  usageSource: 'usage.field.usage_source',
};

export const DEFAULT_VISIBLE_COLUMNS: EventColumn[] = [
  'time',
  'logicalModel',
  'upstreamModel',
  'provider',
  'status',
  'retries',
  'latency',
  'tokens',
];


export const normalizeVisibleEventColumns = (value: unknown): EventColumn[] => {
  if (!Array.isArray(value)) return DEFAULT_VISIBLE_COLUMNS;
  const normalized = EVENT_COLUMNS.filter((column) => value.includes(column));
  return normalized.length > 0 ? normalized : DEFAULT_VISIBLE_COLUMNS;
};

export const loadVisibleColumns = (): EventColumn[] => {
  try {
    return normalizeVisibleEventColumns(JSON.parse(readLocalPreference(COLUMNS_STORAGE_KEY)));
  } catch {
    return DEFAULT_VISIBLE_COLUMNS;
  }
};
