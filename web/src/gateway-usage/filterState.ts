import { readLocalPreference } from '@/lib/browserStorage';
import type { GatewayUsageFilters, UsageRelativePreset } from './types';

export const FILTER_STORAGE_KEY = 'my-ai-gateway-usage-filters-v2';
const DAY_MS = 24 * 60 * 60 * 1000;
const ROLLING_DAYS = { '24h': 1, '7d': 7, '30d': 30 } as const;
const PRESETS: readonly UsageRelativePreset[] = ['today', 'yesterday', '24h', '7d', '30d'];
const ADVANCED_FILTER_FIELDS = [
  'upstreamModel', 'sourceId', 'account', 'clientSource',
  'protocolIn', 'protocolUpstream', 'virtualKey', 'usageSource',
] as const satisfies ReadonlyArray<keyof GatewayUsageFilters>;
const TEXT_FILTER_FIELDS = ['logicalModel', 'provider', ...ADVANCED_FILTER_FIELDS] as const;

export function computeRelativeWindow(preset: UsageRelativePreset, now: Date = new Date()): { from: string; to: string } {
  if (preset === 'today' || preset === 'yesterday') {
    // Construct calendar boundaries independently: DST days can be 23 or 25 hours.
    const offset = preset === 'yesterday' ? -1 : 0;
    const from = new Date(now.getFullYear(), now.getMonth(), now.getDate() + offset);
    const to = new Date(now.getFullYear(), now.getMonth(), now.getDate() + offset + 1);
    return { from: from.toISOString(), to: to.toISOString() };
  }
  return { from: new Date(now.getTime() - ROLLING_DAYS[preset] * DAY_MS).toISOString(), to: now.toISOString() };
}

export function defaultFilters(now: Date = new Date()): GatewayUsageFilters {
  return { ...computeRelativeWindow('today', now), timeMode: 'relative', relativePreset: 'today' };
}

export function resolveFilterWindow(filters: GatewayUsageFilters, now: Date = new Date()): GatewayUsageFilters {
  if (filters.timeMode === 'absolute') return filters;
  const preset = filters.relativePreset ?? 'today';
  return { ...filters, ...computeRelativeWindow(preset, now), timeMode: 'relative', relativePreset: preset };
}

export function serializeFiltersForStorage(filters: GatewayUsageFilters): Partial<GatewayUsageFilters> {
  if (filters.timeMode === 'relative') {
    const { from: _from, to: _to, ...rest } = filters;
    return rest;
  }
  return { ...filters };
}

export function countActiveAdvancedFilters(draft: GatewayUsageFilters): number {
  return ADVANCED_FILTER_FIELDS.filter((field) => Boolean(draft[field])).length;
}

export function isValidTimeRange(from: string, to: string): boolean {
  const fromMs = Date.parse(from);
  const toMs = Date.parse(to);
  return Number.isFinite(fromMs) && Number.isFinite(toMs) && fromMs < toMs;
}

export function safeParseFilters(
  readStorage: () => string = () => readLocalPreference(FILTER_STORAGE_KEY),
  now: Date = new Date(),
): GatewayUsageFilters {
  const fallback = defaultFilters(now);
  try {
    const value: unknown = JSON.parse(readStorage());
    if (!value || typeof value !== 'object' || Array.isArray(value)) return fallback;
    const saved = value as Record<string, unknown>;
    const filters = { ...fallback };
    for (const field of TEXT_FILTER_FIELDS) {
      if (typeof saved[field] === 'string' && saved[field].trim()) filters[field] = saved[field].trim();
    }
    if (saved.status === 'success' || saved.status === 'failure') filters.status = saved.status;
    if (saved.timeMode === 'relative' && PRESETS.includes(saved.relativePreset as UsageRelativePreset)) {
      return resolveFilterWindow({ ...filters, relativePreset: saved.relativePreset as UsageRelativePreset }, now);
    }
    if (saved.timeMode === 'absolute' && typeof saved.from === 'string' && typeof saved.to === 'string' && isValidTimeRange(saved.from, saved.to)) {
      return { ...filters, timeMode: 'absolute', from: saved.from, to: saved.to };
    }
    return filters;
  } catch {
    return fallback;
  }
}
