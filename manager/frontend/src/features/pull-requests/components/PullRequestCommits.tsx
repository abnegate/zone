import { useState } from 'react';
import { usePullRequestCommits } from '../hooks';
import type { PullRequest } from '../types';
import { CommitDiff } from './CommitDiff';
import { CommitRow } from './CommitRow';
import { PullRequestLoading } from './PullRequestLoading';

export const INCOMPLETE_COMMITS = 'GitHub listed only some of the commits on this pull request.';

export const NO_COMMITS = 'No commits on this pull request';

export interface PullRequestCommitsProps {
  pullRequest: PullRequest;
}

export function PullRequestCommits({ pullRequest }: PullRequestCommitsProps) {
  const commits = usePullRequestCommits(pullRequest, true);
  const [selected, setSelected] = useState<string | null>(null);

  if (commits.loading) return <PullRequestLoading label="Loading commits" />;
  if (commits.error) {
    return (
      <div className="error-banner" role="alert">
        {commits.error}
      </div>
    );
  }
  const listed = commits.data?.commits ?? [];
  if (listed.length === 0) return <p className="pull-request-muted">{NO_COMMITS}</p>;

  const choose = (sha: string) => setSelected((current) => (current === sha ? null : sha));

  return (
    <div className="pull-request-commits">
      {commits.data && !commits.data.complete && (
        <p className="source-notice" role="note">
          {INCOMPLETE_COMMITS}
        </p>
      )}
      <ul className="pull-request-commit-list" aria-label="Commits">
        {listed.map((commit) => (
          <CommitRow
            key={commit.sha}
            commit={commit}
            selected={commit.sha === selected}
            onSelect={choose}
          />
        ))}
      </ul>
      {selected && <CommitDiff pullRequest={pullRequest} sha={selected} />}
    </div>
  );
}
