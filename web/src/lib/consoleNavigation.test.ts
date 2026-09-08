import { describe, expect, it } from 'vitest';
import { CONSOLE_PAGES, consolePageHash, isUsagePage, resolveConsolePage } from './consoleNavigation';

describe('console navigation', () => {
  it('resolves the nine production navigation hashes', () => {
    expect(CONSOLE_PAGES).toEqual(['overview', 'analysis', 'events', 'runtime-events', 'sources', 'discovery', 'models', 'capabilities', 'settings']);
    for (const page of CONSOLE_PAGES) expect(resolveConsolePage(consolePageHash(page))).toBe(page);
    expect(resolveConsolePage('#/events/')).toBe('events');
  });
  it('separates usage and management pages using the same route model', () => {
    expect(CONSOLE_PAGES.filter(isUsagePage)).toEqual(['overview', 'analysis', 'events']);
    expect(CONSOLE_PAGES.filter(page => !isUsagePage(page))).toEqual(['runtime-events', 'sources', 'discovery', 'models', 'capabilities', 'settings']);
  });
  it('canonicalizes unknown and removed routes to overview', () => {
    for (const hash of ['', '#ranking', '#management/sources', '#unknown', '#/api/v1']) expect(resolveConsolePage(hash)).toBe('overview');
  });
});
