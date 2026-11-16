import { describe, expect, it } from 'bun:test';

const NOVEMBER_FIRST = new Date('2027-11-01T00:00:00Z');

describe('the test locale', () => {
  it('formats with en-US when no locale is given', () => {
    expect(new Intl.DateTimeFormat().resolvedOptions().locale).toBe('en-US');
    expect(Intl.NumberFormat().resolvedOptions().locale).toBe('en-US');
    expect(NOVEMBER_FIRST.toLocaleDateString(undefined, { timeZone: 'UTC' })).toBe('11/1/2027');
    expect((1234.5).toLocaleString()).toBe('1,234.5');
  });

  it('still honours a locale that is given', () => {
    expect(new Intl.DateTimeFormat('de-DE').resolvedOptions().locale).toBe('de-DE');
    expect(NOVEMBER_FIRST.toLocaleDateString('de-DE', { timeZone: 'UTC' })).toBe('1.11.2027');
    expect((1234.5).toLocaleString('de-DE')).toBe('1.234,5');
  });

  it('keeps formatters recognisable as their own type', () => {
    expect(new Intl.DateTimeFormat()).toBeInstanceOf(Intl.DateTimeFormat);
  });
});
