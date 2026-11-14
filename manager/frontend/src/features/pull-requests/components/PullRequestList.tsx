import { Badge } from '@zone/ui';
import type { PullRequest } from '../types';
import { type PullRequestGroup, pullRequestKey } from '../utils';
import { PullRequestCard } from './PullRequestCard';

export function pullRequestCount(count: number): string {
  return `${count} ${count === 1 ? 'pull request' : 'pull requests'}`;
}

export interface PullRequestListProps {
  groups: readonly PullRequestGroup[];
  selected: string | null;
  onSelect: (pullRequest: PullRequest) => void;
}

export function PullRequestList({ groups, selected, onSelect }: PullRequestListProps) {
  return (
    <>
      {groups.map((group) => (
        <section
          key={group.readiness}
          className="pull-request-group"
          aria-label={group.label}
          data-readiness={group.readiness}
        >
          <h2 className="pull-request-group-header">
            <span className="pull-request-group-label">{group.label}</span>
            <Badge variant={group.tint}>
              <span aria-hidden="true">{group.pullRequests.length}</span>
              <span className="sr-only">{pullRequestCount(group.pullRequests.length)}</span>
            </Badge>
          </h2>
          {group.pullRequests.map((pullRequest) => {
            const key = pullRequestKey(pullRequest);
            return (
              <PullRequestCard
                key={key}
                pullRequest={pullRequest}
                selected={key === selected}
                onSelect={onSelect}
              />
            );
          })}
        </section>
      ))}
    </>
  );
}
