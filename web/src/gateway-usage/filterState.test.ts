import { describe, expect, it } from 'vitest';
import {
  computeRelativeWindow,
  defaultFilters,
  resolveFilterWindow,
  safeParseFilters,
  serializeFiltersForStorage,
} from './filterState';

describe('gateway usage filter state', () => {
  const now = new Date('2026-09-03T12:00:00.000Z');

  it('defaultFilters selects the current local calendar day', () => {
    const filters = defaultFilters(now);
    expect(filters.timeMode).toBe('relative');
    expect(filters.relativePreset).toBe('today');
    expect(filters.from).toBe(new Date(now.getFullYear(), now.getMonth(), now.getDate()).toISOString());
    expect(filters.to).toBe(new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1).toISOString());
  });

  it('resolveFilterWindow recomputes rolling window for relative mode', () => {
    const later = new Date('2026-09-04T12:00:00.000Z');
    const filters = resolveFilterWindow({
      timeMode: 'relative',
      relativePreset: '7d',
      from: '2020-01-01T00:00:00.000Z',
      to: '2020-01-02T00:00:00.000Z',
    }, later);

    expect(filters.from).toBe(new Date(later.getTime() - 7 * 24 * 60 * 60 * 1000).toISOString());
    expect(filters.to).toBe(later.toISOString());
  });

  it('resolveFilterWindow preserves absolute custom ranges', () => {
    const absolute = {
      timeMode: 'absolute' as const,
      from: '2026-01-01T00:00:00.000Z',
      to: '2026-01-02T00:00:00.000Z',
    };
    expect(resolveFilterWindow(absolute, now)).toEqual(absolute);
  });

  it('safeParseFilters falls back to today for a missing time mode', () => {
    const stored = JSON.stringify({
      from: '2020-01-01T00:00:00.000Z',
      to: '2020-01-02T00:00:00.000Z',
      provider: 'openai',
    });
    const parsed = safeParseFilters(() => stored, now);
    expect(parsed.timeMode).toBe('relative');
    expect(parsed.relativePreset).toBe('today');
    expect(parsed.provider).toBe('openai');
    expect(parsed.to).toBe(computeRelativeWindow('today', now).to);
  });

  it('safeParseFilters recomputes relative preset from storage', () => {
    const stored = JSON.stringify({
      timeMode: 'relative',
      relativePreset: '30d',
      logicalModel: 'gpt-4',
    });
    const parsed = safeParseFilters(() => stored, now);
    expect(parsed.relativePreset).toBe('30d');
    expect(parsed.logicalModel).toBe('gpt-4');
    expect(parsed.from).toBe(computeRelativeWindow('30d', now).from);
  });

  it('safeParseFilters preserves absolute mode timestamps', () => {
    const stored = JSON.stringify({
      timeMode: 'absolute',
      from: '2026-01-01T00:00:00.000Z',
      to: '2026-01-02T00:00:00.000Z',
    });
    const parsed = safeParseFilters(() => stored, now);
    expect(parsed.timeMode).toBe('absolute');
    expect(parsed.from).toBe('2026-01-01T00:00:00.000Z');
    expect(parsed.to).toBe('2026-01-02T00:00:00.000Z');
  });

  it('serializeFiltersForStorage omits absolute timestamps in relative mode', () => {
    expect(serializeFiltersForStorage({
      timeMode: 'relative',
      relativePreset: '24h',
      from: '2026-01-01T00:00:00.000Z',
      to: '2026-01-02T00:00:00.000Z',
      provider: 'kimi',
    })).toEqual({
      timeMode: 'relative',
      relativePreset: '24h',
      provider: 'kimi',
    });
  });
});

describe('calendar windows and stored filter validation', () => {
  it('uses adjacent local midnights for today and yesterday, including month and year boundaries', () => {
    const now = new Date(2026, 0, 1, 12, 0, 0);
    expect(computeRelativeWindow('today', now)).toEqual({ from: new Date(2026, 0, 1).toISOString(), to: new Date(2026, 0, 2).toISOString() });
    expect(computeRelativeWindow('yesterday', now)).toEqual({ from: new Date(2025, 11, 31).toISOString(), to: new Date(2026, 0, 1).toISOString() });
  });

  it('recomputes saved calendar presets when the local date changes', () => {
    const saved = JSON.stringify({ timeMode: 'relative', relativePreset: 'yesterday' });
    const now = new Date(2026, 8, 9, 0, 1);
    expect(safeParseFilters(() => saved, now)).toMatchObject({ from: new Date(2026, 8, 8).toISOString(), to: new Date(2026, 8, 9).toISOString(), relativePreset: 'yesterday' });
  });

  it('keeps calendar dates aligned across DST and keeps rolling 24h exactly 24 hours', () => {
    for (const now of [new Date(2026, 2, 8, 12), new Date(2026, 10, 1, 12)]) {
      const calendar = computeRelativeWindow('today', now);
      expect(new Date(calendar.from).getHours()).toBe(0);
      expect(new Date(calendar.to).getHours()).toBe(0);
      expect(new Date(calendar.to).getDate()).toBe(now.getDate() + 1);
      const rolling = computeRelativeWindow('24h', now);
      expect(Date.parse(rolling.to) - Date.parse(rolling.from)).toBe(86_400_000);
      if (process.env.TZ === 'America/New_York') {
        expect(Date.parse(calendar.to) - Date.parse(calendar.from)).toBe((now.getMonth() === 2 ? 23 : 25) * 3_600_000);
      }
    }
  });

  it('rejects corrupt values, unknown presets, and reversed or invalid custom ranges', () => {
    const now = new Date(2026, 8, 8, 12);
    for (const saved of [null, [], { timeMode: 'relative', relativePreset: 'invalid' }, { timeMode: 'absolute', from: 'oops', to: 'now' }, { timeMode: 'absolute', from: '2026-09-09T00:00:00Z', to: '2026-09-08T00:00:00Z' }]) {
      expect(safeParseFilters(() => JSON.stringify(saved), now)).toEqual(defaultFilters(now));
    }
    expect(safeParseFilters(() => { throw new Error('Storage denied'); }, now)).toEqual(defaultFilters(now));
    expect(safeParseFilters(() => '{broken', now)).toEqual(defaultFilters(now));
    expect(safeParseFilters(() => JSON.stringify({ provider: 42, sourceId: {}, status: 'invented' }), now)).toEqual(defaultFilters(now));
  });
});
