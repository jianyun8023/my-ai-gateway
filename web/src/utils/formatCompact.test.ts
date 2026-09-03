import { describe, expect, it } from 'vitest';
import { formatCompact, formatCompactWithTitle, formatExactInteger } from './formatCompact';

describe('formatCompact', () => {
  it('formats values below 10,000 with en-US grouping', () => {
    expect(formatCompact(0)).toBe('0');
    expect(formatCompact(1234)).toBe('1,234');
    expect(formatCompact(9999)).toBe('9,999');
    expect(formatCompact(-4321)).toBe('-4,321');
  });

  it('formats K suffix with one decimal below 100K', () => {
    expect(formatCompact(10_000)).toBe('10.0K');
    expect(formatCompact(12_500)).toBe('12.5K');
    expect(formatCompact(99_999)).toBe('100.0K');
  });

  it('formats K suffix without decimal at 100K and above', () => {
    expect(formatCompact(100_000)).toBe('100K');
    expect(formatCompact(999_999)).toBe('1000K');
  });

  it('formats M suffix with one decimal below 100M', () => {
    expect(formatCompact(1_000_000)).toBe('1.0M');
    expect(formatCompact(12_500_000)).toBe('12.5M');
    expect(formatCompact(99_999_999)).toBe('100.0M');
  });

  it('formats M suffix without decimal at 100M and above', () => {
    expect(formatCompact(100_000_000)).toBe('100M');
    expect(formatCompact(160_000_000)).toBe('160M');
    expect(formatCompact(999_999_999)).toBe('1000M');
  });

  it('formats B suffix with one decimal below 100B', () => {
    expect(formatCompact(1_000_000_000)).toBe('1.0B');
    expect(formatCompact(1_200_000_000)).toBe('1.2B');
    expect(formatCompact(99_999_999_999)).toBe('100.0B');
  });

  it('formats B suffix without decimal at 100B and above', () => {
    expect(formatCompact(100_000_000_000)).toBe('100B');
    expect(formatCompact(999_999_999_999)).toBe('1000B');
  });

  it('formats T suffix with one decimal', () => {
    expect(formatCompact(1_000_000_000_000)).toBe('1.0T');
    expect(formatCompact(2_500_000_000_000)).toBe('2.5T');
  });
});

describe('formatCompactWithTitle', () => {
  it('returns compact display and exact en-US title value', () => {
    expect(formatCompactWithTitle(160_000_000)).toEqual({
      display: '160M',
      exact: '160,000,000',
    });
    expect(formatCompactWithTitle(1234)).toEqual({
      display: '1,234',
      exact: '1,234',
    });
  });
});

describe('formatExactInteger', () => {
  it('formats integers without compact suffixes', () => {
    expect(formatExactInteger(250)).toBe('250');
    expect(formatExactInteger(15000)).toBe('15,000');
  });
});
