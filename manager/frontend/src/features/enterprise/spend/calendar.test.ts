import { afterAll, describe, expect, it } from 'bun:test';

const Native = Intl.DateTimeFormat;

class Joined extends Native {
  override formatToParts(date?: Date | number): Intl.DateTimeFormatPart[] {
    const parts = super.formatToParts(date);
    return parts.map((current, index) => {
      if (current.type !== 'literal') {
        return current;
      }
      if (current.value === ' at ') {
        return { ...current, value: ', ' };
      }
      if (parts[index + 1]?.type === 'dayPeriod') {
        return { ...current, value: ' ' };
      }
      return current;
    });
  }

  override format(date?: Date | number): string {
    return this.formatToParts(date)
      .map((current) => current.value)
      .join('');
  }
}

Intl.DateTimeFormat = Joined as typeof Intl.DateTimeFormat;

const { momentLabel } = await import('./calendar');

afterAll(() => {
  Intl.DateTimeFormat = Native;
});

describe('momentLabel under another ICU', () => {
  it('joins the date and the time in its own words', () => {
    const combined = new Intl.DateTimeFormat('en-US', {
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit',
      timeZone: 'UTC',
      timeZoneName: 'short',
    });
    expect(combined.format(new Date('2026-11-21T10:04:00Z'))).not.toContain(' at ');
    expect(momentLabel('2026-11-21T10:04:00Z')).toBe('Nov 21 at 10:04 AM UTC');
  });
});
