import { describe, expect, it } from 'bun:test';
import { release } from '../../../test/releases';
import { dayLabel, groupReleasesByDay, TODAY, YESTERDAY } from './grouping';

const now = new Date(2026, 10, 13, 15, 30);

function at(day: number, hour = 12, month = 10, year = 2026): string {
  return new Date(year, month, day, hour).toISOString();
}

describe('dayLabel', () => {
  it('names the current day Today', () => {
    expect(dayLabel(at(13, 0), now)).toBe(TODAY);
  });

  it('names the previous calendar day Yesterday, even under 24 hours ago', () => {
    expect(dayLabel(at(12, 23), now)).toBe(YESTERDAY);
    expect(dayLabel(at(12, 1), now)).toBe(YESTERDAY);
  });

  it('names a day earlier in the week by its weekday', () => {
    expect(dayLabel(at(11), now)).toBe('Wednesday');
    expect(dayLabel(at(7), now)).toBe('Saturday');
  });

  it('dates a day a week or more ago so two weekdays never collide', () => {
    expect(dayLabel(at(6), now)).toBe('Friday 6 November');
  });

  it('adds the year to a day in another year', () => {
    expect(dayLabel(at(30, 12, 11, 2025), now)).toBe('Tuesday 30 December 2025');
  });

  it('calls a time later today, or slightly ahead of the clock, Today', () => {
    expect(dayLabel(at(13, 23), now)).toBe(TODAY);
    expect(dayLabel(at(14, 0), now)).toBe(TODAY);
  });

  it('returns a value it cannot read unchanged', () => {
    expect(dayLabel('not a date', now)).toBe('not a date');
  });
});

describe('groupReleasesByDay', () => {
  it('groups releases by calendar day, newest day and newest release first', () => {
    const morning = release({ id: 1, tag: 'v1.0.1', published_at: at(13, 9) });
    const afternoon = release({ id: 2, tag: 'v1.0.2', published_at: at(13, 14) });
    const yesterday = release({ id: 3, tag: 'v1.0.0', published_at: at(12, 18) });
    const days = groupReleasesByDay([yesterday, morning, afternoon], now);
    expect(days.map((day) => day.label)).toEqual([TODAY, YESTERDAY]);
    expect(days[0].releases.map((entry) => entry.id)).toEqual([2, 1]);
    expect(days[1].releases.map((entry) => entry.id)).toEqual([3]);
  });

  it('keys each day by its local date', () => {
    const days = groupReleasesByDay([release({ published_at: at(11) })], now);
    expect(days[0].key).toBe('2026-11-11');
  });

  it('returns no days for no releases', () => {
    expect(groupReleasesByDay([], now)).toEqual([]);
  });

  it('puts a release with an unreadable date last, in its own group', () => {
    const unreadable = release({ id: 9, published_at: 'unknown' });
    const today = release({ id: 1, published_at: at(13) });
    const days = groupReleasesByDay([unreadable, today], now);
    expect(days.map((day) => day.label)).toEqual([TODAY, 'unknown']);
  });

  it('leaves the given list in its order', () => {
    const older = release({ id: 1, published_at: at(12) });
    const newer = release({ id: 2, published_at: at(13) });
    const given = [older, newer];
    groupReleasesByDay(given, now);
    expect(given).toEqual([older, newer]);
  });
});
