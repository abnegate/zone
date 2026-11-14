import { describe, expect, it } from 'bun:test';
import { absoluteTime, instant, relativeTime } from './time';

const NOW = new Date('2026-11-13T12:00:00Z');

describe('relative time', () => {
  it.each([
    ['2026-11-13T11:59:50Z', 'less than a minute ago'],
    ['2026-11-13T11:55:00Z', '5 minutes ago'],
    ['2026-11-13T09:00:00Z', 'about 3 hours ago'],
    ['2026-11-11T12:00:00Z', '2 days ago'],
    ['2026-11-13T12:10:00Z', 'in 10 minutes'],
  ])('describes %s as %s', (value, expected) => {
    expect(relativeTime(value, NOW)).toBe(expected);
  });

  it('measures against the current time by default', () => {
    expect(relativeTime(new Date(Date.now() - 60 * 60 * 1000).toISOString())).toBe(
      'about 1 hour ago'
    );
  });

  it.each(['', 'not a date', '2026-13-45T99:00:00Z'])(
    'hands back %p unchanged rather than throwing',
    (value) => {
      expect(relativeTime(value, NOW)).toBe(value);
    }
  );
});

describe('absolute time', () => {
  it('spells out the date and the time of day', () => {
    const text = absoluteTime('2026-11-13T12:00:00Z');
    expect(text).toMatch(/^Nov 1[34], 2026, \d{1,2}:\d{2}:\d{2} [AP]M$/);
  });

  it('agrees with the local clock', () => {
    const value = '2026-11-13T12:34:56Z';
    const date = new Date(value);
    const hour = date.getHours() % 12 || 12;
    const minute = String(date.getMinutes()).padStart(2, '0');
    expect(absoluteTime(value)).toContain(`${hour}:${minute}:56`);
  });

  it.each(['', 'yesterday'])('hands back %p unchanged rather than throwing', (value) => {
    expect(absoluteTime(value)).toBe(value);
  });
});

describe('instant', () => {
  it('reads an ISO timestamp', () => {
    expect(instant('2026-11-13T12:00:00Z')?.toISOString()).toBe('2026-11-13T12:00:00.000Z');
  });

  it.each(['', 'yesterday', '2026-13-45'])('reads %p as no instant', (value) => {
    expect(instant(value)).toBeUndefined();
  });
});
