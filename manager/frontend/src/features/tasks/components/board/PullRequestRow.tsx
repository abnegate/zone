import type { PullRequest } from '../../PullRequest';
import { CardSensor } from './CardSensor';
import { LinkIcon } from './LinkIcon';
import { PullRequestIcon } from './PullRequestIcon';

export interface PullRequestRowProps {
  pullRequest: PullRequest;
}

const LABEL = 'Pull request';

const UNTINTED = 'untinted';

export function PullRequestRow({ pullRequest }: PullRequestRowProps) {
  const number = pullRequest.number ? `#${pullRequest.number}` : null;
  const className = `task-card-pull-request task-card-pull-request-${pullRequest.status ?? UNTINTED}`;
  const content = (
    <>
      <span className="task-card-link-label">{LABEL}</span>
      <PullRequestIcon />
      {number && <span className="task-card-pull-request-number">{number}</span>}
      {pullRequest.status && (
        <span className="task-card-pull-request-status">{pullRequest.status}</span>
      )}
    </>
  );

  if (pullRequest.address === null) return <span className={className}>{content}</span>;
  return (
    <a
      className={className}
      href={pullRequest.address}
      target="_blank"
      rel="noopener noreferrer"
      aria-label={[LABEL, number, pullRequest.status].filter(Boolean).join(' ')}
      onPointerDown={CardSensor.shield}
      onClick={CardSensor.shield}
      onKeyDown={CardSensor.shield}
    >
      {content}
      <LinkIcon />
    </a>
  );
}
