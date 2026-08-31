import { describe, expect, it } from 'vitest';
import { adaptUsageBreakdown, adaptUsageEventPage, adaptUsageSummary, adaptUsageTimeseries } from './adapter';
import { gatewayUsageBreakdownFixture, gatewayUsageEventsFixture, gatewayUsageSummaryFixture, gatewayUsageTimeseriesFixture } from './fixtures';

describe('gateway usage adapter', () => {
  it('keeps logical requests separate from upstream attempts and tokens', () => {
    const summary = adaptUsageSummary(gatewayUsageSummaryFixture);
    expect(summary.logicalRequests).toBe(3);
    expect(summary.upstreamAttempts).toBe(5);
    expect(summary.retries).toBe(2);
    expect(summary.averageLatencyMs).toBe(940);
    expect(summary.p95LatencyMs).toBe(1280);
    expect(summary.tokens).toEqual({ input: 1200, output: 420, reasoning: 160, cached: 300, total: 1780 });
    expect(summary.usageSources).toEqual({ upstream: 1, estimated: 1, missing: 1 });
  });

  it('adapts timeseries and breakdowns without any pricing dependency', () => {
    expect(adaptUsageTimeseries(gatewayUsageTimeseriesFixture)).toHaveLength(2);
    const breakdown = adaptUsageBreakdown(gatewayUsageBreakdownFixture);
    expect(breakdown[0]).toMatchObject({ key: 'reasoning-large', logicalRequests: 3, upstreamAttempts: 5, averageLatencyMs: 940 });
    expect(breakdown[0].tokens.total).toBe(1780);
  });

  it('marks estimated/missing usage and preserves fallback attribution', () => {
    const page = adaptUsageEventPage(gatewayUsageEventsFixture);
    expect(page.hasMore).toBe(true);
    expect(page.nextCursor).toBe('cursor-2');
    expect(page.events[0]).toMatchObject({ usageSource: 'estimated', retryCount: 1, fallback: true, account: 'fallback-account' });
    expect(page.events[0].attempts.map((attempt) => attempt.statusCode)).toEqual([429, 200]);
    expect(page.events[1]).toMatchObject({ usageSource: 'missing', success: false, tokens: { total: 0 } });
  });

  it('also accepts the current flat E4.1a event shape during #15 integration', () => {
    const page = adaptUsageEventPage({ events: [{ request_id: 'r1', created_at: '2026-08-30T00:00:00Z', model: 'm', source: 'client-a', status_code: 200, success: true, input_tokens: 2, output_tokens: 3, total_tokens: 5 }] });
    expect(page.events[0]).toMatchObject({ requestId: 'r1', logicalModel: 'm', source: 'client-a', tokens: { input: 2, output: 3, total: 5 } });
  });

  it('matches the versioned #15 data/page envelope', () => {
    const summary = adaptUsageSummary({
      version: 'v1',
      timezone: 'UTC',
      data: {
        logical_requests: 4,
        upstream_attempts: 6,
        retries: 2,
        successes: 3,
        failures: 1,
        input_tokens: 10,
        output_tokens: 5,
        reasoning_tokens: 2,
        cached_tokens: 1,
        total_tokens: 17,
      },
    });
    expect(summary).toMatchObject({ logicalRequests: 4, upstreamAttempts: 6, retries: 2, successfulRequests: 3, failedRequests: 1, tokens: { total: 17 } });

    const page = adaptUsageEventPage({
      version: 'v1',
      data: [{ request_id: 'r2', created_at: '2026-08-30T00:00:00Z', logical_model: 'm', status_code: 500, success: false }],
      page: { limit: 100, has_more: true, next_cursor: 'cursor-r2' },
    });
    expect(page).toMatchObject({ hasMore: true, nextCursor: 'cursor-r2' });
  });
});
