import { describe, expect, it } from 'vitest';
import { GATEWAY_USAGE_TABS, resolveGatewayUsageTab } from '../pages/GatewayUsagePage';

describe('gateway usage navigation', () => {
  it('exposes exactly the three first-release pages', () => {
    expect(GATEWAY_USAGE_TABS).toEqual(['overview', 'analysis', 'events']);
  });

  it('uses hash routes so the /admin static mount supports reloads', () => {
    expect(resolveGatewayUsageTab('#analysis')).toBe('analysis');
    expect(resolveGatewayUsageTab('#/events')).toBe('events');
    expect(resolveGatewayUsageTab('#ranking')).toBe('overview');
    expect(resolveGatewayUsageTab('#auth-files')).toBe('overview');
  });
});
