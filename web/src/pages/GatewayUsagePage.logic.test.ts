import { describe, expect, it } from 'vitest';
import { adaptUsageEventPage } from '@/gateway-usage';
import { gatewayUsageEventsFixture } from '@/gateway-usage/fixtures';
import { appendStableEventPage, normalizeVisibleEventColumns, resolveGatewayUsageTab } from './GatewayUsagePage';

describe('GatewayUsagePage logic', () => {
  it('falls unknown/CPA routes back to Overview', () => {
    expect(resolveGatewayUsageTab('#overview')).toBe('overview');
    expect(resolveGatewayUsageTab('#events')).toBe('events');
    expect(resolveGatewayUsageTab('#quota')).toBe('overview');
  });

  it('persists only supported columns and never allows an empty table', () => {
    expect(normalizeVisibleEventColumns(['time', 'clientSource', 'usageSource', 'cost'])).toEqual(['time', 'clientSource', 'usageSource']);
    expect(normalizeVisibleEventColumns([]).length).toBeGreaterThan(0);
  });

  it('appends stable cursor pages without duplicate events', () => {
    const events = adaptUsageEventPage(gatewayUsageEventsFixture).events;
    expect(appendStableEventPage(events, [events[0], { ...events[1], id: 'evt-3' }])).toHaveLength(3);
  });
});
