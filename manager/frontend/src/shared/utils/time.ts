import { format, formatDistance, isValid, parseISO } from 'date-fns';

const ABSOLUTE_PATTERN = 'PPpp';

export function instant(value: string): Date | undefined {
  const date = parseISO(value);
  return isValid(date) ? date : undefined;
}

export function relativeTime(value: string, now: Date = new Date()): string {
  const date = instant(value);
  return date ? formatDistance(date, now, { addSuffix: true }) : value;
}

export function absoluteTime(value: string): string {
  const date = instant(value);
  return date ? format(date, ABSOLUTE_PATTERN) : value;
}
