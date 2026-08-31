import { describe, expect, it } from 'vitest';
import {
  consoleRouteHash,
  GATEWAY_MANAGEMENT_PAGES,
  GATEWAY_USAGE_TABS,
  resolveConsoleRoute,
} from '../lib/consoleNavigation';

describe('gateway usage navigation', () => {
  it('exposes exactly the three first-release pages', () => {
    expect(GATEWAY_USAGE_TABS).toEqual(['overview', 'analysis', 'events']);
  });

  it('uses hash routes so the /admin static mount supports reloads', () => {
    expect(resolveConsoleRoute('#analysis')).toEqual({ space: 'usage', page: 'analysis' });
    expect(resolveConsoleRoute('#/events')).toEqual({ space: 'usage', page: 'events' });
    expect(resolveConsoleRoute('#ranking')).toEqual({ space: 'usage', page: 'overview' });
    expect(resolveConsoleRoute('#auth-files')).toEqual({ space: 'usage', page: 'overview' });
  });

  it('keeps Management in an independent hash namespace', () => {
    expect(GATEWAY_MANAGEMENT_PAGES).toEqual(['sources', 'model-discovery', 'capabilities']);
    expect(resolveConsoleRoute('#management/capabilities')).toEqual({ space: 'management', page: 'capabilities' });
    expect(consoleRouteHash({ space: 'management', page: 'sources' })).toBe('#management/sources');
  });
});
