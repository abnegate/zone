import { differenceInCalendarDays, format } from 'date-fns';
import { instant } from '../../../shared/utils/time';
import type { Release } from '../types';

export const TODAY = 'Today';
export const YESTERDAY = 'Yesterday';

const WEEK_DAYS = 7;
const DAY_KEY_PATTERN = 'yyyy-MM-dd';
const WEEKDAY_PATTERN = 'EEEE';
const DATE_PATTERN = 'EEEE d MMMM';
const DATED_YEAR_PATTERN = 'EEEE d MMMM yyyy';

export interface ReleaseDay {
  key: string;
  label: string;
  releases: Release[];
}

export function dayLabel(value: string, now: Date = new Date()): string {
  const date = instant(value);
  if (!date) return value;
  const days = differenceInCalendarDays(now, date);
  if (days <= 0) return TODAY;
  if (days === 1) return YESTERDAY;
  if (days < WEEK_DAYS) return format(date, WEEKDAY_PATTERN);
  const pattern = date.getFullYear() === now.getFullYear() ? DATE_PATTERN : DATED_YEAR_PATTERN;
  return format(date, pattern);
}

function newestFirst(left: Release, right: Release): number {
  const leftTime = Date.parse(left.published_at);
  const rightTime = Date.parse(right.published_at);
  if (Number.isNaN(leftTime)) return Number.isNaN(rightTime) ? 0 : 1;
  if (Number.isNaN(rightTime)) return -1;
  return rightTime - leftTime;
}

export function groupReleasesByDay(
  releases: readonly Release[],
  now: Date = new Date()
): ReleaseDay[] {
  const days = new Map<string, ReleaseDay>();
  for (const release of [...releases].sort(newestFirst)) {
    const date = instant(release.published_at);
    const key = date ? format(date, DAY_KEY_PATTERN) : release.published_at;
    const day = days.get(key);
    if (day) {
      day.releases.push(release);
    } else {
      days.set(key, { key, label: dayLabel(release.published_at, now), releases: [release] });
    }
  }
  return [...days.values()];
}
