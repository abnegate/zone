import { absoluteTime, relativeTime } from '../../../shared/utils/time';
import type { BotComment } from '../types';
import { GitHubLink } from './GitHubLink';

export interface BotCommentRowProps {
  comment: BotComment;
}

export function BotCommentRow({ comment }: BotCommentRowProps) {
  return (
    <li className="pull-request-item">
      <div className="pull-request-row">
        <span className="pull-request-row-title">{comment.reviewer}</span>
        <time
          className="pull-request-row-time"
          dateTime={comment.created_at}
          title={absoluteTime(comment.created_at)}
        >
          {relativeTime(comment.created_at)}
        </time>
        <GitHubLink href={comment.url} label={`Open the ${comment.reviewer} review on GitHub`} />
      </div>
      <p className="pull-request-quote">{comment.body}</p>
    </li>
  );
}
