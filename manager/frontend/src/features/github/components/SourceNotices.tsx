import { cn } from '@zone/ui';
import { UNAVAILABLE } from '../notices';
import './SourceNotices.css';

export const NOTICES_LABEL = 'Notices';

export interface SourceNoticesProps {
  failure: string | null;
  unavailable: boolean;
  notices: readonly string[];
  className?: string;
}

export function SourceNotices({ failure, unavailable, notices, className }: SourceNoticesProps) {
  if (!failure && !unavailable && notices.length === 0) return null;
  return (
    <div className={cn('source-notices', className)}>
      {failure && (
        <div className="error-banner" role="alert">
          {failure}
        </div>
      )}
      {unavailable && (
        <p className="source-notice" role="status">
          {UNAVAILABLE}
        </p>
      )}
      {notices.length > 0 && (
        <ul className="source-notice-list" aria-label={NOTICES_LABEL}>
          {notices.map((notice) => (
            <li key={notice} className="source-notice">
              {notice}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
