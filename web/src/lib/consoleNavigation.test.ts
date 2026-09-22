import { describe, expect, it } from 'vitest';
import {
  canonicalConsoleHash,
  CONSOLE_PAGES,
  consolePageHash,
  isUsagePage,
  modelRouteHash,
  resolveConsolePage,
  resolveConsoleRoute,
  runtimeEventsRouteHash,
  sourceRouteHash,
  upstreamQuotaRouteHash,
  usageEventRouteHash,
} from './consoleNavigation';

describe('console navigation', () => {
  it('resolves the eight production navigation hashes', () => {
    expect(CONSOLE_PAGES).toEqual(['overview', 'analysis', 'events', 'upstream-quotas', 'runtime-events', 'sources', 'models', 'settings']);
    for (const page of CONSOLE_PAGES) expect(resolveConsolePage(consolePageHash(page))).toBe(page);
    expect(resolveConsolePage('#/events/')).toBe('events');
  });
  it('separates usage and management pages using the same route model', () => {
    expect(CONSOLE_PAGES.filter(isUsagePage)).toEqual(['overview', 'analysis', 'events']);
    expect(CONSOLE_PAGES.filter(page => !isUsagePage(page))).toEqual(['upstream-quotas', 'runtime-events', 'sources', 'models', 'settings']);
  });
  it('resolves the source workspace sub routes', () => {
    expect(resolveConsoleRoute('#sources')).toEqual({ page: 'sources' });
    expect(resolveConsoleRoute('#sources/deepseek')).toEqual({ page: 'sources', sourceId: 'deepseek' });
    expect(resolveConsoleRoute('#sources/deepseek/edit')).toEqual({ page: 'sources', sourceId: 'deepseek', section: 'edit' });
    expect(resolveConsoleRoute('#sources/deepseek/review')).toEqual({ page: 'sources', sourceId: 'deepseek', section: 'review' });
    expect(resolveConsoleRoute('#sources/new/edit')).toEqual({ page: 'sources', sourceId: 'new', section: 'edit' });
    expect(canonicalConsoleHash('#/sources/deepseek/unknown/')).toBe('#sources/deepseek');
    expect(sourceRouteHash('deep seek', 'review')).toBe('#sources/deep%20seek/review');
  });
  it('resolves upstream quota list and account detail routes', () => {
    expect(resolveConsoleRoute('#upstream-quotas')).toEqual({ page: 'upstream-quotas' });
    expect(resolveConsoleRoute('#upstream-quotas/kimi-main')).toEqual({ page: 'upstream-quotas', accountId: 'kimi-main' });
    expect(canonicalConsoleHash('#/upstream-quotas/kimi%20main/')).toBe('#upstream-quotas/kimi%20main');
    expect(upstreamQuotaRouteHash('kimi main')).toBe('#upstream-quotas/kimi%20main');
  });
  it('round-trips focused runtime-event and model routes', () => {
    expect(resolveConsoleRoute('#runtime-events?correlation_id=request%2F1')).toEqual({ page: 'runtime-events', correlationId: 'request/1' });
    expect(runtimeEventsRouteHash('request/1')).toBe('#runtime-events?correlation_id=request%2F1');
    expect(resolveConsoleRoute('#models?model=logical%20one')).toEqual({ page: 'models', modelSearch: 'logical one' });
    expect(modelRouteHash('logical one')).toBe('#models?model=logical%20one');
    expect(canonicalConsoleHash('#models?model=logical%20one')).toBe('#models?model=logical%20one');
  });
  it('round-trips a focused request with encoded IDs', () => {
    const requestId = 'request/1 ?&';
    const hash = '#events?request_id=request%2F1%20%3F%26';
    expect(usageEventRouteHash(requestId)).toBe(hash);
    expect(resolveConsoleRoute(hash)).toEqual({ page: 'events', requestId });
    expect(canonicalConsoleHash('#/events/?request_id=request%2F1%20%3F%26')).toBe(hash);
    expect(resolveConsoleRoute('#events')).toEqual({ page: 'events', requestId: undefined });
  });
  it('canonicalizes unknown and removed routes to overview', () => {
    for (const hash of ['', '#ranking', '#management/sources', '#unknown', '#/api/v1', '#discovery', '#capabilities']) expect(resolveConsolePage(hash)).toBe('overview');
  });
});
