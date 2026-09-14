// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from '@/test/render';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setTestLanguage } from '@/test/setup';
import { adaptUsageBreakdown, adaptUsageEventPage, adaptUsageSummary } from '@/gateway-usage/adapter';
import { gatewayUsageSummaryFixture, gatewayUsageSourcesFixture, gatewayUsageEventsFixture } from '@/test/fixtures/usage';
import type { UsageSummaryViewModel } from '@/gateway-usage';
import { Overview } from './UsageOverview';

vi.mock('react-chartjs-2', () => ({ Bar: () => null, Line: () => null }));
const baseline = adaptUsageSummary(gatewayUsageSummaryFixture, adaptUsageBreakdown(gatewayUsageSourcesFixture));
const zeroTokens = { input: 0, output: 0, total: 0, reasoning: 0, cached: 0, cacheRead: 0, cacheCreation: 0 };
describe('overview metrics and usage provenance', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  beforeEach(async () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    await setTestLanguage('en');
    container = document.createElement('div'); document.body.appendChild(container); root = createRoot(container);
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); });
  const render = (summary: UsageSummaryViewModel) => act(() => root.render(<Overview data={{ summary, timeseries: [], logicalModels: [], recentEvents: adaptUsageEventPage(gatewayUsageEventsFixture).events }} metric="composition" onMetricChange={() => {}} />));
  const metric = (index: number) => container.querySelectorAll('[data-ui="metric-card"]')[index];

  it('shows real zero latency, missing latency and exact large values', () => {
    render({ ...baseline, averageLatencyMs: 0, p95LatencyMs: undefined, logicalRequests: 12345678 });
    expect(metric(0).querySelector('strong')?.textContent).toBe('12.3M');
    expect(metric(0).querySelector('strong')?.getAttribute('aria-label')).toBe('12,345,678');
    expect(metric(3).textContent).toContain('0 ms');
    expect(metric(3).textContent).toContain('P95 —');
    render({ ...baseline, averageLatencyMs: undefined });
    expect(metric(3).querySelector('strong')?.textContent).toBe('—');
  });
  it('preserves confirmed zero tokens but does not call all-missing accounting zero confirmed', () => {
    render({ ...baseline, tokens: zeroTokens, usageSources: { upstream: 3 } });
    expect(metric(2).querySelector('strong')?.textContent).toBe('0');
    render({ ...baseline, tokens: zeroTokens, usageSources: { missing: 3 } });
    expect(metric(2).querySelector('strong')?.textContent).toBe('—');
    expect(container.textContent).toContain('accounting zero is not a confirmed zero');
    expect(metric(1).querySelector('strong')?.textContent).toBe('66.7%');
    expect(container.textContent).toContain('Usage is missing for 3 requests');
    render({ ...baseline, tokens: zeroTokens, usageSources: { unknown: 3 } });
    expect(metric(2).querySelector('strong')?.textContent).toBe('—');
    expect(container.textContent).toContain('unknown usage source');
  });
  it('explains mixed estimates/missing and provides text statuses and provenance for recent requests', () => {
    render(baseline);
    expect(container.textContent).toContain('1 requests use estimated tokens');
    expect(container.textContent).toContain('Usage is missing for 1 requests');
    const recent = container.querySelector('[data-od-id="recent-activity"]')!;
    expect(recent.textContent).toContain('Success'); expect(recent.textContent).toContain('Failure');
    expect(recent.textContent).toContain('Estimated'); expect(recent.textContent).toContain('Not captured');
    expect(recent.textContent).toContain('— tokens');
    expect(recent.textContent).toContain('source-tokyo');
    expect(metric(2).querySelector('strong')?.getAttribute('title')).toBe('1,780');
  });
  it('shows the no-request empty state without fabricated successful or token KPIs', () => {
    render({ ...baseline, logicalRequests: 0, tokens: zeroTokens, usageSources: {} });
    expect(container.querySelector('[data-ui="metric-card"]')).toBeNull();
    expect(container.textContent).toContain('No usage in the selected range');
  });
});
