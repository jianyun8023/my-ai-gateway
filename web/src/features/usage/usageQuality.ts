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

interface ThroughputTiming {
  latencyMs?: number;
  ttftMs?: number;
  streamed: boolean;
}

// Generation window used as the TPS denominator. For streaming requests the
// queue/prefill time before the first token is excluded (latency − TTFT);
// otherwise the full end-to-end latency is the best available window.
// Returns null when there is no trustworthy positive window.
export const generationTimeMs = (event: ThroughputTiming): number | null => {
  if (event.latencyMs === undefined || !Number.isFinite(event.latencyMs) || event.latencyMs <= 0) return null;
  if (event.streamed && event.ttftMs !== undefined && Number.isFinite(event.ttftMs)) {
    const window = event.latencyMs - event.ttftMs;
    if (window > 0) return window;
  }
  return event.latencyMs;
};

// Output tokens per second over the generation window. Returns null when the
// output side is unknown so the UI shows '—' instead of a misleading rate.
export const outputTokensPerSecond = (event: ThroughputTiming & {
  tokens: Pick<TokenTotals, 'output'>;
  usageSource: string;
}): number | null => {
  if (isUnreportedUsage(event.usageSource)) return null;
  if (!Number.isFinite(event.tokens.output) || event.tokens.output <= 0) return null;
  const windowMs = generationTimeMs(event);
  if (windowMs === null) return null;
  return event.tokens.output / (windowMs / 1000);
};
