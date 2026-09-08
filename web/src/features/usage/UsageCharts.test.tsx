// @vitest-environment happy-dom
import { act, useState } from 'react';
import { createRoot } from '@/test/render';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setTestLanguage } from '@/test/setup';
import type { UsageBreakdownItem, UsageSummaryViewModel } from '@/gateway-usage';
import { UsageTrend } from './UsageTrend';
import { TokenDistribution } from './TokenDistribution';
import { TokenComposition } from './TokenComposition';
import { Analysis } from './UsageAnalysis';
import type { TrendMetric } from './model';

vi.mock('react-chartjs-2', () => ({
  Bar: ({ data, options }: { data: unknown; options: unknown }) => <output data-chart="bar">{JSON.stringify({ data, options })}</output>,
  Line: ({ data, options }: { data: unknown; options: unknown }) => <output data-chart="line">{JSON.stringify({ data, options })}</output>,
}));
const tokens = { input: 1200, output: 420, total: 1780, reasoning: 160, cached: 900, cacheRead: 900, cacheCreation: 0 };
const summary: UsageSummaryViewModel = { tokens, logicalRequests: 20, successfulRequests: 18, failedRequests: 2, successRate: .9, upstreamAttempts: 22, retries: 2, averageLatencyMs: 300, p95LatencyMs: 800, usageSources: { missing: 2, upstream: 18 } };
const row = (label: string, total: number, averageLatencyMs?: number): UsageBreakdownItem => ({ key: label, label, tokens: { ...tokens, total }, logicalRequests: 12, upstreamAttempts: 12, successfulRequests: 12, averageLatencyMs });

describe('usage charts and accessible data', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  beforeEach(async () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    await setTestLanguage('en');
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); });
  const click = (text: string) => act(() => [...container.querySelectorAll('button')].find(button => button.textContent === text)!.click());

  it('stacks only input/output, preserves reported total on metric change and exposes exact tabular values', () => {
    function Probe() {
      const [metric, setMetric] = useState<TrendMetric>('composition');
      return <UsageTrend metric={metric} onMetricChange={setMetric} points={[{ bucket: '2026-09-08T00:00:00Z', tokens, logicalRequests: 20, upstreamAttempts: 22, successfulRequests: 18 }]} />;
    }
    act(() => root.render(<Probe />));
    const chart = () => JSON.parse(container.querySelector('output')!.textContent!);
    expect(chart().data.datasets.map((dataset: { data: number[] }) => dataset.data)).toEqual([[1200], [420]]);
    expect(chart().options.scales.y.stacked).toBe(true);
    click('View data');
    expect([...container.querySelectorAll('tbody td')].map(cell => cell.textContent).slice(1)).toEqual(['1,200', '420']);
    act(() => {
      const select = container.querySelector('select')!;
      select.value = 'total';
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(container.querySelector('[data-chart="line"]')).not.toBeNull();
    expect(chart().data.datasets.map((dataset: { data: number[] }) => dataset.data)).toEqual([[1780], [20]]);
    expect(chart().options.scales.y.title.text).toBe('Total Tokens');
    expect(chart().options.scales.secondary.title.text).toBe('Logical Requests');
    expect(container.querySelector('table')!.textContent).toContain('1,780');
    click('Hide data');
    expect(container.querySelector('table')).toBeNull();
  });

  it('sorts distribution rows against the full total and represents zero without a fabricated minimum', () => {
    act(() => root.render(<TokenDistribution rows={[row('zero', 0), row('small', 100), row('largest', 600)]} total={1000} limit={2} />));
    expect([...container.querySelectorAll('strong')].map(el => el.textContent)).toEqual(['largest', 'small']);
    expect(container.textContent).toContain('60.0%');
    expect(container.textContent).toContain('10.0%');
    expect(container.textContent).toContain('top 2 of 3');
    act(() => root.render(<TokenDistribution rows={[row('zero', 0)]} total={1000} />));
    expect(container.textContent).toContain('0.0%');
    expect(container.querySelector('[role="progressbar"]')?.getAttribute('aria-valuenow')).toBe('0');
    act(() => root.render(<TokenDistribution rows={[row('unknown denominator', 1)]} total={0} />));
    expect(container.textContent).toContain('—');
    expect(container.textContent).not.toContain('Infinity');
  });

  it('keeps overlapping token categories independent, includes zero and explains missing usage', () => {
    act(() => root.render(<TokenComposition summary={summary} />));
    expect(container.textContent).toContain('1,780');
    expect(container.textContent).toContain('50.6%');
    expect(container.textContent).toContain('0.0%');
    expect(container.textContent).toContain('2 requests');
    expect(container.querySelectorAll('[role="progressbar"]')).toHaveLength(5);
    act(() => root.render(<TokenComposition summary={{ ...summary, tokens: { ...tokens, input: 0, total: 0 } }} />));
    expect(container.textContent).toContain('—');
    expect(container.textContent).not.toMatch(/NaN|Infinity/);
  });

  it('compares latency only within sources, distinguishes zero from missing and retains every source', () => {
    act(() => root.render(<Analysis summary={summary} breakdowns={{ provider: [row('excluded provider', 500, 999)], source_id: [row('missing source', 0), { ...row('zero source', 100, 0), p95LatencyMs: 0 }, { ...row('slow source', 600, 900), p95LatencyMs: 1500 }] }} />));
    const rows = [...container.querySelectorAll('tbody tr')].map(el => el.textContent);
    expect(rows).toEqual(['slow source900 ms1.5 s12', 'zero source0 ms0 ms12', 'missing source——12']);
    expect(container.querySelector('table')!.textContent).not.toContain('excluded provider');
  });

  it('shows an explicit empty trend rather than a blank canvas', () => {
    act(() => root.render(<UsageTrend points={[]} metric="composition" onMetricChange={() => {}} />));
    expect(container.querySelector('output')).toBeNull();
    expect(container.querySelector('select')).not.toBeNull();
    expect(container.textContent).toContain('No trend data');
  });
});
