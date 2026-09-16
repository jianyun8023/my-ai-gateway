import { describe, expect, it } from 'vitest';
import {
  canonicalConsoleHash,
  CONSOLE_PAGES,
  consolePageHash,
  isUsagePage,
  resolveConsolePage,
  resolveConsoleRoute,
  sourceRouteHash,
  upstreamQuotaRouteHash,
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
  it('canonicalizes unknown and removed routes to overview', () => {
    for (const hash of ['', '#ranking', '#management/sources', '#unknown', '#/api/v1', '#discovery', '#capabilities']) expect(resolveConsolePage(hash)).toBe('overview');
  });
});
