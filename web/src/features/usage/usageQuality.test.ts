import { cacheHitRate, hasOnlyUnreportedUsage, isUnreportedUsage, outputTokensPerSecond } from './usageQuality';
import { describe, expect, it } from 'vitest';

describe('outputTokensPerSecond', () => {
  const event = {
    latencyMs: 5526, ttftMs: 5522, streamed: true,
    tokens: { output: 31, reasoning: 10 }, usageSource: 'parsed',
  };

  it.each([
    [31, 5526, 5522, 5.6098],
    [229, 8183, 8082, 27.9848],
    [196, 7173, 7081, 27.3247],
  ])('uses total latency for production timing: %i tokens in %i ms', (output, latencyMs, ttftMs, expected) => {
    const sample = { ...event, latencyMs, ttftMs, tokens: { ...event.tokens, output } };
    expect(outputTokensPerSecond(sample)).toBeCloseTo(expected, 3);
  });

  it.each([undefined, 0, 5522, 5526, 6000])('does not depend on first-data timing (%s)', (ttftMs) => {
    const sample = { ...event, ttftMs };
    const nonStreaming = { ...sample, streamed: false };
    expect(outputTokensPerSecond(sample)).toBeCloseTo(5.6098, 3);
    expect(outputTokensPerSecond(nonStreaming)).toBeCloseTo(5.6098, 3);
  });

  it('uses full output tokens without subtracting or adding reasoning tokens', () => {
    expect(outputTokensPerSecond(event)).toBeCloseTo(31 / 5.526);
  });

  it.each([undefined, 0, -1, Number.NaN, Number.POSITIVE_INFINITY])('omits speed for invalid latency (%s)', (latencyMs) => {
    expect(outputTokensPerSecond({ ...event, latencyMs })).toBeNull();
  });

  it.each([0, -1, Number.NaN, Number.POSITIVE_INFINITY])('omits speed for invalid output (%s)', (output) => {
    expect(outputTokensPerSecond({ ...event, tokens: { output } })).toBeNull();
  });

  it.each(['missing', 'unknown'])('omits speed when usage is %s', (usageSource) => {
    expect(outputTokensPerSecond({ ...event, usageSource })).toBeNull();
  });
});

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
