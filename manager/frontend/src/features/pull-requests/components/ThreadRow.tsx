import { Badge } from '@zone/ui';
import type { Thread } from '../types';
import { GitHubLink } from './GitHubLink';

export const CONVERSATION = 'Conversation';

function location(thread: Thread): string {
  if (!thread.path) return CONVERSATION;
  return thread.line === null ? thread.path : `${thread.path}:${thread.line}`;
}

export interface ThreadRowProps {
  thread: Thread;
}

export function ThreadRow({ thread }: ThreadRowProps) {
  const [first] = thread.comments;
  const replies = thread.comments.length - 1;
  return (
    <li className="pull-request-item">
      <div className="pull-request-row">
        <code className="pull-request-row-title pull-request-path">{location(thread)}</code>
        {thread.outdated && <Badge variant="neutral">Outdated</Badge>}
        {thread.url && <GitHubLink href={thread.url} label="Open thread on GitHub" />}
      </div>
      {first && (
        <p className="pull-request-quote">
          <span className="pull-request-quote-author">{first.author}</span>
          {first.body}
        </p>
      )}
      {replies > 0 && (
        <p className="pull-request-muted">
          {replies} {replies === 1 ? 'reply' : 'replies'}
        </p>
      )}
    </li>
  );
}
