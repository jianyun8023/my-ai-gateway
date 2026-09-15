import { readLocalPreference } from '@/lib/browserStorage';

// Bump the suffix when the column set changes so stored preferences reset to
// defaults that include the new columns.
export const COLUMNS_STORAGE_KEY = 'my-ai-gateway-usage-event-columns-v4';

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
  'tps',
  'cache',
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
  tps: 'usage.field.tps',
  cache: 'usage.field.cache',
  usageSource: 'usage.field.usage_source',
};

// Token, throughput and cache cells carry right-aligned numeric metrics.
export const NUMERIC_EVENT_COLUMNS: ReadonlySet<EventColumn> = new Set(['tokens', 'tps', 'cache']);

// Column headers that explain the displayed metric through an info tooltip.
export const EVENT_COLUMN_HINTS: Partial<Record<EventColumn, string>> = {
  tokens: 'usage.detail.token_subtitle',
  tps: 'usage.events.tps_basis',
  cache: 'usage.events.cache_basis',
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
  'tps',
  'cache',
];

// Grid track per column: text columns flex to fill the viewport, while compact
// metric columns (status/retries/latency/tokens/tps/cache) are capped so the
// numeric cluster stays tight instead of stretching across wide screens.
interface EventColumnTrack { min: number; max: string }

export const EVENT_COLUMN_TRACKS: Record<EventColumn, EventColumnTrack> = {
  time: { min: 150, max: '1.1fr' },
  logicalModel: { min: 140, max: '1.3fr' },
  upstreamModel: { min: 140, max: '1.3fr' },
  provider: { min: 110, max: '1fr' },
  sourceAccount: { min: 120, max: '1fr' },
  clientSource: { min: 110, max: '1fr' },
  protocol: { min: 120, max: '1fr' },
  status: { min: 116, max: '0.7fr' },
  retries: { min: 84, max: '108px' },
  latency: { min: 76, max: '96px' },
  tokens: { min: 88, max: '112px' },
  tps: { min: 84, max: '104px' },
  cache: { min: 84, max: '104px' },
  usageSource: { min: 128, max: '1fr' },
};

const EVENT_ACTIONS_TRACK_WIDTH = 80;

export const eventTableGridTemplate = (columns: EventColumn[]): string => [
  `${EVENT_ACTIONS_TRACK_WIDTH}px`,
  ...columns.map((column) => {
    const track = EVENT_COLUMN_TRACKS[column];
    return `minmax(${track.min}px, ${track.max})`;
  }),
].join(' ');

export const eventTableMinWidth = (columns: EventColumn[]): number =>
  EVENT_ACTIONS_TRACK_WIDTH + columns.reduce((total, column) => total + EVENT_COLUMN_TRACKS[column].min, 0);

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
