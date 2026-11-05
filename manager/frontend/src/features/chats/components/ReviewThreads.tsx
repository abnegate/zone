import type { PullRequestThreads } from '../types';
import { threadsLabel } from '../utils/pullRequests';

const OPEN_ICON = 'M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12Z';
const RESOLVED_ICON = 'm5 12.5 4.5 4.5L19 7.5';

function ThreadIcon({ path }: { path: string }) {
  return (
    <svg
      className="chat-change-threads-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.25"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={path} />
    </svg>
  );
}

export function ReviewThreads({ threads }: { threads: PullRequestThreads }) {
  if (threads.open + threads.resolved === 0) return null;
  const label = threadsLabel(threads);
  return (
    <span className="chat-change-threads" role="img" aria-label={label} title={label}>
      <span className={threads.open > 0 ? 'chat-change-threads-open' : undefined}>
        <ThreadIcon path={OPEN_ICON} />
        {threads.open}
      </span>
      <span>
        <ThreadIcon path={RESOLVED_ICON} />
        {threads.resolved}
      </span>
    </span>
  );
}
