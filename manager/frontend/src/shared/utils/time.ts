import { format, formatDistance, isValid, parseISO } from 'date-fns';

const ABSOLUTE_PATTERN = 'PPpp';

const ZONED = /(?:Z|[+-]\d{2}:?\d{2})$/i;

export function instant(value: string): Date | undefined {
  const date = parseISO(value);
  return isValid(date) ? date : undefined;
}

export function utcInstant(value: string): Date | undefined {
  return instant(ZONED.test(value) ? value : `${value}Z`);
}

export function relativeTime(value: string, now: Date = new Date()): string {
  const date = instant(value);
  return date ? formatDistance(date, now, { addSuffix: true }) : value;
}

export function absoluteTime(value: string): string {
  const date = instant(value);
  return date ? format(date, ABSOLUTE_PATTERN) : value;
}
