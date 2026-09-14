import { cacheHitRate, hasOnlyUnreportedUsage, isUnreportedUsage } from './usageQuality';
import { describe, expect, it } from 'vitest';

describe('isUnreportedUsage', () => {
  it('treats upstream-reported and estimated usage as reported', () => {
    expect(isUnreportedUsage('upstream')).toBe(false);
    expect(isUnreportedUsage('parsed')).toBe(false);
    expect(isUnreportedUsage('estimated')).toBe(false);
    expect(isUnreportedUsage('missing')).toBe(true);
    expect(isUnreportedUsage('unknown')).toBe(true);
  });
});

describe('cacheHitRate', () => {
  it('computes cache read over input', () => {
    expect(cacheHitRate({ input: 23373, cacheRead: 22528 })).toBeCloseTo(0.9638, 3);
    expect(cacheHitRate({ input: 100, cacheRead: 0 })).toBe(0);
  });

  it('returns null when input is not positive so the UI shows a dash', () => {
    expect(cacheHitRate({ input: 0, cacheRead: 0 })).toBeNull();
    expect(cacheHitRate({ input: 0, cacheRead: 50 })).toBeNull();
    expect(cacheHitRate({ input: -5, cacheRead: 10 })).toBeNull();
    expect(cacheHitRate({ input: Number.NaN, cacheRead: 10 })).toBeNull();
  });

  it('clamps abnormal values into [0, 1]', () => {
    expect(cacheHitRate({ input: 100, cacheRead: 250 })).toBe(1);
    expect(cacheHitRate({ input: 100, cacheRead: -10 })).toBe(0);
    expect(cacheHitRate({ input: 100, cacheRead: Number.NaN })).toBe(0);
  });
});

describe('hasOnlyUnreportedUsage', () => {
  const summary = {
    logicalRequests: 2,
    successfulRequests: 1,
    failedRequests: 1,
    successRate: 0.5,
    upstreamAttempts: 2,
    retries: 0,
    tokens: { input: 0, output: 0, reasoning: 0, cached: 0, cacheRead: 0, cacheCreation: 0, total: 0 },
    usageSources: { missing: 2 },
  };

  it('flags zero totals fully backed by unreported usage', () => {
    expect(hasOnlyUnreportedUsage(summary)).toBe(true);
    expect(hasOnlyUnreportedUsage({ ...summary, usageSources: { upstream: 2 } })).toBe(false);
    expect(hasOnlyUnreportedUsage({ ...summary, tokens: { ...summary.tokens, total: 10 } })).toBe(false);
  });
});
