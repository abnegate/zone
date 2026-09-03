import { describe, expect, it } from 'bun:test';
import { formatBytes, formatContextLength, formatDate, formatNumber } from './formatters';

describe('formatNumber', () => {
  it('formats thousands and millions', () => {
    expect(formatNumber(950)).toBe('950');
    expect(formatNumber(2800)).toBe('2.8K');
    expect(formatNumber(1_400_000)).toBe('1.4M');
  });
});

describe('formatBytes', () => {
  it('formats byte sizes', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(3800000000)).toBe('3.5 GB');
    expect(formatBytes(4100000000)).toBe('3.8 GB');
  });
});

describe('formatContextLength', () => {
  it('formats token windows', () => {
    expect(formatContextLength(262144)).toBe('256K');
    expect(formatContextLength(1048576)).toBe('1M');
    expect(formatContextLength(128000)).toBe('128K');
    expect(formatContextLength(131072)).toBe('128K');
    expect(formatContextLength(512)).toBe('512');
  });
});

describe('formatDate', () => {
  it('returns the original string for invalid dates', () => {
    expect(formatDate('5 days ago')).toBe('5 days ago');
  });
});
