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

  it('defaultFilters returns relative 24h mode', () => {
    const filters = defaultFilters(now);
    expect(filters.timeMode).toBe('relative');
    expect(filters.relativePreset).toBe('24h');
    expect(filters.to).toBe(now.toISOString());
    expect(filters.from).toBe(new Date(now.getTime() - 24 * 60 * 60 * 1000).toISOString());
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

  it('safeParseFilters migrates old localStorage format to relative 24h', () => {
    const stored = JSON.stringify({
      from: '2020-01-01T00:00:00.000Z',
      to: '2020-01-02T00:00:00.000Z',
      provider: 'openai',
    });
    const parsed = safeParseFilters(() => stored, now);
    expect(parsed.timeMode).toBe('relative');
    expect(parsed.relativePreset).toBe('24h');
    expect(parsed.provider).toBe('openai');
    expect(parsed.to).toBe(now.toISOString());
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
