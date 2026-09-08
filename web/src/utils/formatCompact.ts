/**
 * Format large numbers with K/M/B/T suffixes consistently across all locales.
 * Rules:
 * - 0..9,999 → locale separator (e.g., "1,234")
 * - >= 10,000 → K/M/B/T with max 1 decimal (e.g., "160M", "1.2B")
 * - Percentages → 1 decimal
 * - Latency → ms/s, NOT K/M/B
 */
export function formatCompact(value: number): string {
  const abs = Math.abs(value);
  if (abs < 10_000) {
    return new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 }).format(value);
  }
  if (abs < 1_000_000) {
    return `${(value / 1_000).toFixed(abs < 100_000 ? 1 : 0)}K`;
  }
  if (abs < 1_000_000_000) {
    return `${(value / 1_000_000).toFixed(abs < 100_000_000 ? 1 : 0)}M`;
  }
  if (abs < 1_000_000_000_000) {
    return `${(value / 1_000_000_000).toFixed(abs < 100_000_000_000 ? 1 : 0)}B`;
  }
  return `${(value / 1_000_000_000_000).toFixed(1)}T`;
}

/** Format with exact value in title attribute */
export function formatCompactWithTitle(value: number): { display: string; exact: string } {
  return {
    display: formatCompact(value),
    exact: new Intl.NumberFormat('en-US').format(value),
  };
}

/** Format exact integer values for latency ms display (not K/M/B). */
export function formatExactInteger(value: number): string {
  return new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 }).format(value);
}

/** Input is a ratio, e.g. 0.125 → 12.5%. */
export function formatPercent(value?: number | null): string {
  return value == null || !Number.isFinite(value) ? '—' : `${(value * 100).toFixed(1)}%`;
}
