export interface StampProps {
  value: string;
  now: number;
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const MONTH = 30 * DAY;
const YEAR = 365 * DAY;

const SPANS = [
  { unit: 'year', length: YEAR, suffix: 'y' },
  { unit: 'month', length: MONTH, suffix: 'mo' },
  { unit: 'day', length: DAY, suffix: 'd' },
  { unit: 'hour', length: HOUR, suffix: 'h' },
  { unit: 'minute', length: MINUTE, suffix: 'm' },
] as const;

const SPOKEN = new Intl.RelativeTimeFormat('en', { numeric: 'always' });

const JUST_NOW = { brief: 'now', spoken: 'just now' };

function age(elapsed: number): { brief: string; spoken: string } {
  const span = SPANS.find(({ length }) => elapsed >= length);
  if (!span) return JUST_NOW;
  const count = Math.floor(elapsed / span.length);
  return { brief: `${count}${span.suffix}`, spoken: SPOKEN.format(-count, span.unit) };
}

export function Stamp({ value, now }: StampProps) {
  const time = new Date(value).getTime();
  if (Number.isNaN(time)) return null;
  const { brief, spoken } = age(now - Math.min(time, now));
  return (
    <time className="task-card-time" dateTime={value}>
      <span aria-hidden="true">{brief}</span>
      <span className="sr-only">{spoken}</span>
    </time>
  );
}
