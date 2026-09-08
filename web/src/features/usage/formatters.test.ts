import { describe, expect, it } from 'vitest';
import { formatDuration, formatUsageTokens } from './formatters';
import { formatCompact, formatPercent } from '@/utils/formatCompact';

describe('usage numeric formats', () => {
  it('distinguishes zero from absent latency and keeps exact detail milliseconds', () => {
    expect([0, undefined, null, -1, NaN].map(value => formatDuration(value))).toEqual(['0 ms', '—', '—', '—', '—']);
    expect(formatDuration(1280)).toBe('1.3 s');
    expect(formatDuration(1280, true)).toBe('1,280 ms');
    expect(formatDuration(120000)).toBe('120 s');
  });
  it('distinguishes missing zero from reported zero and preserves nonzero unknown counts', () => {
    expect(formatUsageTokens(0, 'missing')).toBe('—');
    expect(formatUsageTokens(0, 'unknown')).toBe('—');
    expect(formatUsageTokens(0, 'upstream')).toBe('0');
    expect(formatUsageTokens(0, 'parsed')).toBe('0');
    expect(formatUsageTokens(12345, 'unknown', true)).toBe('12,345');
  });
  it('preserves K/M/B/T and formats ratios consistently', () => {
    expect([0, 9999, 10000, 1200000, 1200000000, 1200000000000].map(formatCompact)).toEqual(['0', '9,999', '10.0K', '1.2M', '1.2B', '1.2T']);
    expect([0, .12345, 1, undefined].map(formatPercent)).toEqual(['0.0%', '12.3%', '100.0%', '—']);
  });
});
