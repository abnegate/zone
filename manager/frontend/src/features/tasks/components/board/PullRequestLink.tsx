import { Badge } from '@zone/ui';
import type { PullRequest } from '../../PullRequest';
import type { StatusTint } from '../../status';
import type { PrStatus } from '../../types';
import { CardSensor } from './CardSensor';
import { LinkIcon } from './LinkIcon';
import './PullRequestLink.css';

export interface PullRequestLinkProps {
  pullRequest: PullRequest;
}

const TINTS: Record<PrStatus, StatusTint> = {
  pending: 'neutral',
  open: 'success',
  merged: 'accent',
  closed: 'destructive',
};

const UNTINTED: StatusTint = 'neutral';

export function PullRequestLink({ pullRequest }: PullRequestLinkProps) {
  const variant = pullRequest.status ? TINTS[pullRequest.status] : UNTINTED;
  if (pullRequest.address === null) {
    return (
      <Badge className="task-pull-request-status" variant={variant}>
        {pullRequest.label}
      </Badge>
    );
  }
  return (
    <a
      className="task-pull-request"
      href={pullRequest.address}
      target="_blank"
      rel="noopener noreferrer"
      onPointerDown={CardSensor.shield}
      onClick={CardSensor.shield}
      onKeyDown={CardSensor.shield}
    >
      <Badge className="task-pull-request-status" variant={variant}>
        {pullRequest.label}
        <LinkIcon />
      </Badge>
    </a>
  );
}
