import { formatDistance } from 'date-fns';

export interface StampProps {
  value: string;
  now: number;
}

export function Stamp({ value, now }: StampProps) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  return (
    <time className="task-card-time" dateTime={value}>
      {formatDistance(date, now, { addSuffix: true })}
    </time>
  );
}
