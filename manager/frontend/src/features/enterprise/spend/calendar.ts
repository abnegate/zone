const LOCALE = 'en-US';

const TIME_ZONE = 'UTC';

const MONTH_START = '-01T00:00:00Z';

const DAY_START = 'T00:00:00Z';

const days = new Intl.DateTimeFormat(LOCALE, {
  month: 'short',
  day: 'numeric',
  timeZone: TIME_ZONE,
});

const dates = new Intl.DateTimeFormat(LOCALE, {
  month: 'short',
  day: 'numeric',
  year: 'numeric',
  timeZone: TIME_ZONE,
});

const months = new Intl.DateTimeFormat(LOCALE, {
  month: 'long',
  year: 'numeric',
  timeZone: TIME_ZONE,
});

const clock = new Intl.DateTimeFormat(LOCALE, {
  hour: 'numeric',
  minute: '2-digit',
  timeZone: TIME_ZONE,
  timeZoneName: 'short',
});

function formatted(value: string, format: Intl.DateTimeFormat): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : format.format(date);
}

export function dayLabel(date: string): string {
  return formatted(`${date}${DAY_START}`, days);
}

export function dateLabel(date: string): string {
  return formatted(`${date}${DAY_START}`, dates);
}

export function monthLabel(month: string): string {
  return formatted(`${month}${MONTH_START}`, months);
}

function part(parts: Intl.DateTimeFormatPart[], type: Intl.DateTimeFormatPartTypes): string {
  return parts.find((candidate) => candidate.type === type)?.value ?? '';
}

export function momentLabel(instant: string): string {
  const date = new Date(instant);
  if (Number.isNaN(date.getTime())) {
    return instant;
  }
  const time = clock.formatToParts(date);
  return `${days.format(date)} at ${part(time, 'hour')}:${part(time, 'minute')} ${part(time, 'dayPeriod')} ${part(time, 'timeZoneName')}`;
}
