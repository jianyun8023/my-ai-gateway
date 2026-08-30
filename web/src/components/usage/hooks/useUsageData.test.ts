import { describe, expect, it } from 'vitest';
import { normalizeUsageOverviewRange } from './useUsageData';

describe('normalizeUsageOverviewRange', () => {
  it('preserves the 30d and yesterday presets for overview requests', () => {
    expect(normalizeUsageOverviewRange('30d')).toBe('30d');
    expect(normalizeUsageOverviewRange('yesterday')).toBe('yesterday');
  });

  it('falls back to the default required range instead of all data', () => {
    expect(normalizeUsageOverviewRange('all')).toBe('8h');
    expect(normalizeUsageOverviewRange('invalid')).toBe('8h');
  });
});
