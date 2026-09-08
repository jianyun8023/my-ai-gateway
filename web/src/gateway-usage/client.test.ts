import type { AdminTransport } from '@/admin-api/client';
import { AdminClient } from '@/admin-api/client';
import { describe, expect, it, vi } from 'vitest';
import { buildGatewayUsageURL, GatewayUsageClient } from './client';
import type { GatewayUsageFilters } from './types';

const filters: GatewayUsageFilters = {
  from: '2026-08-30T00:00:00.000Z',
  to: '2026-08-31T00:00:00.000Z',
  logicalModel: 'logical-a',
  upstreamModel: 'upstream-a',
  provider: 'provider-a',
  sourceId: 'source-a',
  clientSource: 'codex-desktop',
  account: 'account-a',
  protocolIn: 'openai_responses',
  protocolUpstream: 'anthropic_messages',
  virtualKey: 'key-a',
  status: 'success',
  usageSource: 'estimated',
};

describe('GatewayUsageClient', () => {
  it('serializes the complete combination filter contract', () => {
    const url = buildGatewayUsageURL('events', filters, { cursor: 'next', limit: 100 });
    expect(url).toContain('/admin/usage/events?');
    for (const key of ['from', 'to', 'logical_model', 'upstream_model', 'provider', 'source_id', 'client_source', 'account', 'protocol_in', 'protocol_upstream', 'virtual_key', 'status', 'usage_source', 'cursor', 'limit']) {
      expect(new URL(url, 'http://gateway.local').searchParams.has(key)).toBe(true);
    }
  });

  it('only accesses gateway Admin Usage endpoints', async () => {
    const urls: string[] = [];
    const fetchImpl = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      urls.push(url);
      if (url.includes('/summary')) return new Response(JSON.stringify({ logical_requests: { total: 0 }, tokens: {} }), { status: 200 });
      if (url.includes('/timeseries')) return new Response(JSON.stringify({ items: [] }), { status: 200 });
      if (url.includes('/breakdown')) return new Response(JSON.stringify({ items: [] }), { status: 200 });
      if (url.includes('/export')) return new Response('request_id\n', { status: 200 });
      return new Response(JSON.stringify({ items: [], has_more: false }), { status: 200 });
    });
    const client = new GatewayUsageClient(new AdminClient({ fetchImpl: fetchImpl as typeof fetch, getAdminKey: () => 'secret' }));

    await client.overview(filters);
    await client.breakdown(filters, 'provider');
    await client.events({ filters, limit: 20 });
    await client.exportEvents(filters, 'csv');

    expect(urls.length).toBeGreaterThan(0);
    expect(urls.filter(url => url.includes('breakdown=usage_source'))).toHaveLength(1);
    for (const url of urls) {
      expect(url.startsWith('/admin/usage/')).toBe(true);
      expect(url).not.toMatch(/\/api\/v1|quota|pricing|ranking|auth-files|request-log|management\.html/i);
    }
    expect(urls.some((url) => url.includes('/breakdown?') && url.includes('breakdown=provider'))).toBe(true);
    expect(urls.some((url) => url.includes('/export?') && url.includes('format=csv'))).toBe(true);
    for (const [, init] of fetchImpl.mock.calls as unknown as [string, RequestInit][]) {
      expect(new Headers(init.headers).get('Authorization')).toBe('Bearer secret');
      expect(init.redirect).toBe('error');
    }
  });
});

it('joins real summary and usage_source responses once with identical filters and signal', async () => {
  const { gatewayUsageSummaryFixture, gatewayUsageSourcesFixture } = await import('@/test/fixtures/usage');
  const signal = new AbortController().signal;
  const json = vi.fn(async (url: string) => url.includes('/summary') ? gatewayUsageSummaryFixture : gatewayUsageSourcesFixture);
  const client = new GatewayUsageClient({ json: json as AdminTransport['json'], blob: vi.fn() });
  const summary = await client.summary(filters, signal);
  expect(summary.usageSources).toEqual({ upstream: 1, estimated: 1, missing: 1 });
  expect(summary.logicalRequests).toBe(3);
  expect(json).toHaveBeenCalledTimes(2);
  expect(json).toHaveBeenCalledWith(buildGatewayUsageURL('summary', filters), { signal });
  expect(json).toHaveBeenCalledWith(buildGatewayUsageURL('breakdown', filters, { breakdown: 'usage_source' }), { signal });
});

it.each(['summary', 'breakdown'])('rejects a failed %s instead of publishing fabricated source counts', async (failed) => {
  const json = vi.fn(async (url: string) => { if (url.includes(`/${failed}`)) throw new Error('unavailable'); return { data: [] }; });
  const client = new GatewayUsageClient({ json: json as AdminTransport['json'], blob: vi.fn() });
  await expect(client.summary(filters)).rejects.toThrow('unavailable');
  expect(json).toHaveBeenCalledTimes(2);
});
