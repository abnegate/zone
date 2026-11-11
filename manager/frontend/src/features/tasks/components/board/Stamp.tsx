import { formatDistance } from 'date-fns';

export interface StampProps {
  value: string;
  now: number;
}

export function Stamp({ value, now }: StampProps) {
  const time = new Date(value).getTime();
  if (Number.isNaN(time)) return null;
  return (
    <time className="task-card-time" dateTime={value}>
      {formatDistance(Math.min(time, now), now, { addSuffix: true })}
    </time>
  );
}
