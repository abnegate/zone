import { type FormatDistanceToken, formatDistanceStrict } from 'date-fns';

export interface StampProps {
  value: string;
  now: number;
}

const ROUNDING = 'floor';

const JUST_NOW = 'now';

const SUFFIXES: Partial<Record<FormatDistanceToken, string>> = {
  xMinutes: 'm',
  xHours: 'h',
  xDays: 'd',
  xMonths: 'mo',
  xYears: 'y',
};

const BRIEF = {
  formatDistance: (token: FormatDistanceToken, count: number): string => {
    const suffix = SUFFIXES[token];
    return suffix ? `${count}${suffix}` : JUST_NOW;
  },
};

export function Stamp({ value, now }: StampProps) {
  const time = new Date(value).getTime();
  if (Number.isNaN(time)) return null;
  const moment = Math.min(time, now);
  return (
    <time className="task-card-time" dateTime={value}>
      <span aria-hidden="true">
        {formatDistanceStrict(moment, now, { roundingMethod: ROUNDING, locale: BRIEF })}
      </span>
      <span className="sr-only">
        {formatDistanceStrict(moment, now, { roundingMethod: ROUNDING, addSuffix: true })}
      </span>
    </time>
  );
}
