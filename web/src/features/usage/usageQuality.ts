import type { UsageSummaryViewModel } from '@/gateway-usage';

export const isUnreportedUsage = (source: string) => !['upstream', 'parsed', 'estimated'].includes(source);

// A zero accounting total is not a confirmed zero when every counted request
// lacks usable usage. Summary and breakdown are separate database reads.
export const hasOnlyUnreportedUsage = (summary: UsageSummaryViewModel) => {
  const rows = Object.entries(summary.usageSources).filter(([, count]) => count > 0);
  return summary.logicalRequests > 0 && summary.tokens.total === 0 && rows.length > 0
    && rows.every(([source]) => isUnreportedUsage(source));
};
