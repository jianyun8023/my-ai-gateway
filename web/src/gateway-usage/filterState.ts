import type { GatewayUsageFilters } from './types';

export const FILTER_STORAGE_KEY = 'my-ai-gateway-usage-filters-v2';

export type RelativePreset = '24h' | '7d' | '30d';
export type TimeMode = 'relative' | 'absolute';

const DAY_MS = 24 * 60 * 60 * 1000;

export const RELATIVE_PRESET_MS: Record<RelativePreset, number> = {
  '24h': DAY_MS,
  '7d': 7 * DAY_MS,
  '30d': 30 * DAY_MS,
};

const ADVANCED_FILTER_FIELDS = [
  'upstreamModel',
  'sourceId',
  'account',
  'clientSource',
  'protocolIn',
  'protocolUpstream',
  'virtualKey',
  'usageSource',
] as const satisfies ReadonlyArray<keyof GatewayUsageFilters>;

export function computeRelativeWindow(preset: RelativePreset, now: Date = new Date()): { from: string; to: string } {
  const to = now;
  const from = new Date(to.getTime() - RELATIVE_PRESET_MS[preset]);
  return { from: from.toISOString(), to: to.toISOString() };
}

export function defaultFilters(now: Date = new Date()): GatewayUsageFilters {
  const { from, to } = computeRelativeWindow('24h', now);
  return {
    from,
    to,
    timeMode: 'relative',
    relativePreset: '24h',
  };
}

export function resolveFilterWindow(filters: GatewayUsageFilters, now: Date = new Date()): GatewayUsageFilters {
  if (filters.timeMode === 'absolute') {
    return filters;
  }
  const preset = filters.relativePreset ?? '24h';
  const { from, to } = computeRelativeWindow(preset, now);
  return {
    ...filters,
    from,
    to,
    timeMode: 'relative',
    relativePreset: preset,
  };
}

export function serializeFiltersForStorage(filters: GatewayUsageFilters): Partial<GatewayUsageFilters> {
  if (filters.timeMode === 'relative') {
    const { from: _from, to: _to, timeMode, relativePreset, ...rest } = filters;
    return { timeMode, relativePreset, ...rest };
  }
  return { ...filters };
}

export function countActiveAdvancedFilters(draft: GatewayUsageFilters): number {
  return ADVANCED_FILTER_FIELDS.reduce((count, field) => {
    const value = draft[field];
    return value ? count + 1 : count;
  }, 0);
}

const isValidIsoRange = (from?: string, to?: string): boolean => {
  if (!from || !to) return false;
  const fromMs = Date.parse(from);
  const toMs = Date.parse(to);
  return Number.isFinite(fromMs) && Number.isFinite(toMs);
};

export function safeParseFilters(
  readStorage: () => string = () => {
    try {
      return localStorage.getItem(FILTER_STORAGE_KEY) ?? '';
    } catch {
      return '';
    }
  },
  now: Date = new Date(),
): GatewayUsageFilters {
  const fallback = defaultFilters(now);
  try {
    const raw = readStorage();
    if (!raw) return fallback;
    const value = JSON.parse(raw) as Partial<GatewayUsageFilters>;

    if (!value.timeMode) {
      const { from: _from, to: _to, ...rest } = value;
      return resolveFilterWindow({
        ...fallback,
        ...rest,
        timeMode: 'relative',
        relativePreset: '24h',
      }, now);
    }

    if (value.timeMode === 'relative') {
      return resolveFilterWindow({
        ...fallback,
        ...value,
        timeMode: 'relative',
        relativePreset: value.relativePreset ?? '24h',
      }, now);
    }

    if (!isValidIsoRange(value.from, value.to)) {
      return fallback;
    }
    return {
      ...fallback,
      ...value,
      timeMode: 'absolute',
    };
  } catch {
    return fallback;
  }
}
