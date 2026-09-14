import type { TokenTotals, UsageSummaryViewModel } from '@/gateway-usage';

export const isUnreportedUsage = (source: string) => !['upstream', 'parsed', 'estimated'].includes(source);

// cache_hit_rate = cache_read / input; cache creation never counts as a hit.
// Returns null when input is not positive so the UI shows '—' instead of 0%.
// Clamped to [0, 1] so abnormal upstream data cannot render >100% or negatives.
export const cacheHitRate = (tokens: Pick<TokenTotals, 'input' | 'cacheRead'>): number | null => {
  if (!Number.isFinite(tokens.input) || tokens.input <= 0) return null;
  if (!Number.isFinite(tokens.cacheRead)) return 0;
  return Math.min(Math.max(tokens.cacheRead / tokens.input, 0), 1);
};

// A zero accounting total is not a confirmed zero when every counted request
// lacks usable usage. Summary and breakdown are separate database reads.
export const hasOnlyUnreportedUsage = (summary: UsageSummaryViewModel) => {
  const rows = Object.entries(summary.usageSources).filter(([, count]) => count > 0);
  return summary.logicalRequests > 0 && summary.tokens.total === 0 && rows.length > 0
    && rows.every(([source]) => isUnreportedUsage(source));
};
