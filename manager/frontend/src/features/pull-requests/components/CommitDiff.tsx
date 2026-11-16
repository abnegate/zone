import { useCommitDiff } from '../hooks';
import type { PullRequest } from '../types';
import { PullRequestDiff } from './PullRequestDiff';
import { PullRequestLoading } from './PullRequestLoading';

export interface CommitDiffProps {
  pullRequest: PullRequest;
  sha: string;
}

export function CommitDiff({ pullRequest, sha }: CommitDiffProps) {
  const diff = useCommitDiff(pullRequest, sha);
  if (diff.loading) return <PullRequestLoading label="Loading commit diff" />;
  if (diff.error) {
    return (
      <div className="error-banner" role="alert">
        {diff.error}
      </div>
    );
  }
  return diff.data ? <PullRequestDiff diff={diff.data} /> : null;
}
