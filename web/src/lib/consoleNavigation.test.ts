import { describe, expect, it } from 'vitest';
import {
  consoleRouteHash,
  GATEWAY_MANAGEMENT_PAGES,
  GATEWAY_USAGE_TABS,
  resolveConsoleRoute,
} from './consoleNavigation';

describe('console navigation', () => {
  it('preserves the three first-release Usage hashes', () => {
    expect(GATEWAY_USAGE_TABS).toEqual(['overview', 'analysis', 'events']);
    expect(resolveConsoleRoute('#overview')).toEqual({ space: 'usage', page: 'overview' });
    expect(resolveConsoleRoute('#/events')).toEqual({ space: 'usage', page: 'events' });
    expect(consoleRouteHash({ space: 'usage', page: 'analysis' })).toBe('#analysis');
  });

  it('provides an independent Management namespace', () => {
    expect(GATEWAY_MANAGEMENT_PAGES).toEqual(['sources', 'model-discovery', 'capabilities', 'models-routes', 'settings']);
    expect(resolveConsoleRoute('#management/model-discovery')).toEqual({
      space: 'management',
      page: 'model-discovery',
    });
    expect(consoleRouteHash({ space: 'management', page: 'capabilities' })).toBe('#management/capabilities');
    expect(resolveConsoleRoute('#management/models-routes')).toEqual({ space: 'management', page: 'models-routes' });
    expect(resolveConsoleRoute('#management/settings')).toEqual({ space: 'management', page: 'settings' });
  });

  it('falls unknown and legacy CPA routes back to Usage Overview', () => {
    for (const hash of ['', '#ranking', '#auth-files', '#management', '#management/unknown', '#/api/v1']) {
      expect(resolveConsoleRoute(hash)).toEqual({ space: 'usage', page: 'overview' });
    }
  });
});
