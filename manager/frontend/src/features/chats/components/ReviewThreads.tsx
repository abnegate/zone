import type { PullRequestThreads } from '../types';
import { threadsLabel } from '../utils/pullRequests';
import { MenuIcon } from './MenuIcon';

const COMMENT_ICON = 'M4 4h16v12H7.5L4 19.5V4Z';
const UNRESOLVED_ICON = 'M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12Z';
const RESOLVED_ICON = 'm5 12.5 4.5 4.5L19 7.5';

export interface ReviewThreadsProps {
  comments: number | null;
  threads: PullRequestThreads | null;
}

export function ReviewThreads({ comments, threads }: ReviewThreadsProps) {
  if (comments === null && threads === null) return null;
  const label = threadsLabel({ comments, threads });
  return (
    <span className="chat-change-threads" role="img" aria-label={label} title={label}>
      {comments !== null && (
        <span className="chat-change-threads-comments">
          <MenuIcon path={COMMENT_ICON} className="chat-change-threads-icon" />
          {comments}
        </span>
      )}
      {threads !== null && (
        <>
          <span className={threads.open > 0 ? 'chat-change-threads-open' : undefined}>
            <MenuIcon path={UNRESOLVED_ICON} className="chat-change-threads-icon" />
            {threads.open}
          </span>
          <span className="chat-change-threads-resolved">
            <MenuIcon path={RESOLVED_ICON} className="chat-change-threads-icon" />
            {threads.resolved}
          </span>
        </>
      )}
    </span>
  );
}
