import { usePullRequestDiff } from '../hooks';
import type { PullRequest } from '../types';
import { PullRequestDiff } from './PullRequestDiff';
import { PullRequestLoading } from './PullRequestLoading';

export interface DiffPanelProps {
  pullRequest: PullRequest;
}

export function DiffPanel({ pullRequest }: DiffPanelProps) {
  const diff = usePullRequestDiff(pullRequest, true);
  if (diff.loading) return <PullRequestLoading label="Loading diff" />;
  if (diff.error) {
    return (
      <div className="error-banner" role="alert">
        {diff.error}
      </div>
    );
  }
  return diff.data ? <PullRequestDiff diff={diff.data} /> : null;
}
