import { memberCapMessage } from '../members';
import { PRICING_URL } from '../pricing';

interface MemberCapRefusalProps {
  className: string;
}

export function MemberCapRefusal({ className }: MemberCapRefusalProps) {
  return (
    <div className={className} role="alert">
      {memberCapMessage()}{' '}
      <a href={PRICING_URL} target="_blank" rel="noopener noreferrer">
        Compare plans
        <span className="sr-only"> (opens in a new tab)</span>
      </a>
    </div>
  );
}
