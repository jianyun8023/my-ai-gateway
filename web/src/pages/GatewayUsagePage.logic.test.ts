import { describe, expect, it } from 'vitest';
import { adaptUsageEventPage } from '@/gateway-usage';
import { gatewayUsageEventsFixture } from '@/gateway-usage/fixtures';
import '@/i18n/console';
import i18n from '@/i18n';
import { appendStableEventPage, formatFallbackReason, normalizeVisibleEventColumns } from './GatewayUsagePage';

describe('GatewayUsagePage logic', () => {
  it('persists only supported columns and never allows an empty table', () => {
    expect(normalizeVisibleEventColumns(['time', 'clientSource', 'usageSource', 'cost'])).toEqual(['time', 'clientSource', 'usageSource']);
    expect(normalizeVisibleEventColumns([]).length).toBeGreaterThan(0);
  });

  it('appends stable cursor pages without duplicate events', () => {
    const events = adaptUsageEventPage(gatewayUsageEventsFixture).events;
    expect(appendStableEventPage(events, [events[0], { ...events[1], id: 'evt-3' }])).toHaveLength(3);
  });
});

describe('formatFallbackReason', () => {
  it('localizes known primary-unavailable reason codes', () => {
    const t = i18n.getFixedT('zh', 'console');
    expect(formatFallbackReason(t, 'account_cooling_down')).toBe('主账号冷却中（用量熔断 / 失败退避）');
    expect(formatFallbackReason(t, 'account_disabled')).toBe('主账号已禁用');
    expect(formatFallbackReason(t, 'upstream_transport_error')).toBe('主账号上游连接失败');
  });

  it('templates upstream_http_<status> and keeps unknown codes visible', () => {
    const t = i18n.getFixedT('en', 'console');
    expect(formatFallbackReason(t, 'upstream_http_429')).toBe('Primary upstream returned 429');
    expect(formatFallbackReason(t, 'upstream_http_503')).toBe('Primary upstream returned 503');
    expect(formatFallbackReason(t, 'some_future_code')).toBe('Fallback: some_future_code');
  });
});
