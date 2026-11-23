import { absoluteTime, relativeTime } from '../../../../shared/utils/time';

interface MomentProps {
  value: string;
}

export function Moment({ value }: MomentProps) {
  return (
    <time dateTime={value} title={absoluteTime(value)}>
      {relativeTime(value)}
    </time>
  );
}
