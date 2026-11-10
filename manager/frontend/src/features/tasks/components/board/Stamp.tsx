import { formatDistanceToNow } from 'date-fns';

export interface StampProps {
  value: string;
}

export function Stamp({ value }: StampProps) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  return (
    <time className="task-card-time" dateTime={value} title={date.toLocaleString()}>
      {formatDistanceToNow(date, { addSuffix: true })}
    </time>
  );
}
