import { describe, expect, it } from 'vitest';
import {
  canonicalConsoleHash,
  CONSOLE_PAGES,
  consolePageHash,
  isUsagePage,
  resolveConsolePage,
  resolveConsoleRoute,
  sourceRouteHash,
} from './consoleNavigation';

describe('console navigation', () => {
  it('resolves the eight production navigation hashes', () => {
    expect(CONSOLE_PAGES).toEqual(['overview', 'analysis', 'events', 'runtime-events', 'sources', 'models', 'capabilities', 'settings']);
    for (const page of CONSOLE_PAGES) expect(resolveConsolePage(consolePageHash(page))).toBe(page);
    expect(resolveConsolePage('#/events/')).toBe('events');
  });
  it('separates usage and management pages using the same route model', () => {
    expect(CONSOLE_PAGES.filter(isUsagePage)).toEqual(['overview', 'analysis', 'events']);
    expect(CONSOLE_PAGES.filter(page => !isUsagePage(page))).toEqual(['runtime-events', 'sources', 'models', 'capabilities', 'settings']);
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
  it('canonicalizes unknown and removed routes to overview', () => {
    for (const hash of ['', '#ranking', '#management/sources', '#unknown', '#/api/v1', '#discovery']) expect(resolveConsolePage(hash)).toBe('overview');
  });
});
