import type { PullRequestThreads } from '../types';
import { threadsLabel } from '../utils/pullRequests';
import { MenuIcon } from './MenuIcon';

const UNRESOLVED_ICON = 'M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12Z';
const RESOLVED_ICON = 'm5 12.5 4.5 4.5L19 7.5';

export interface ReviewThreadsProps {
  threads: PullRequestThreads;
}

export function ReviewThreads({ threads }: ReviewThreadsProps) {
  if (threads.open + threads.resolved === 0) return null;
  const label = threadsLabel(threads);
  return (
    <span className="chat-change-threads" role="img" aria-label={label} title={label}>
      <span className={threads.open > 0 ? 'chat-change-threads-open' : undefined}>
        <MenuIcon path={UNRESOLVED_ICON} className="chat-change-threads-icon" />
        {threads.open}
      </span>
      <span className="chat-change-threads-resolved">
        <MenuIcon path={RESOLVED_ICON} className="chat-change-threads-icon" />
        {threads.resolved}
      </span>
    </span>
  );
}
