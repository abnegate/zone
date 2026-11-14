import { absoluteTime, relativeTime } from '../../../shared/utils/time';
import { shortSha } from '../../github';
import type { Commit } from '../types';
import { firstLine } from '../utils';

function author(commit: Commit): string | null {
  return commit.author_login ?? commit.author_name;
}

export interface CommitRowProps {
  commit: Commit;
  selected: boolean;
  onSelect: (sha: string) => void;
}

export function CommitRow({ commit, selected, onSelect }: CommitRowProps) {
  const name = author(commit);
  return (
    <li>
      <button
        type="button"
        className="pull-request-commit"
        aria-pressed={selected}
        onClick={() => onSelect(commit.sha)}
      >
        <code className="pull-request-commit-sha">{shortSha(commit.sha)}</code>
        <span className="pull-request-commit-message" title={commit.message}>
          {firstLine(commit.message)}
        </span>
        <span className="pull-request-commit-meta">
          {name && <span className="pull-request-commit-author">{name}</span>}
          {commit.authored_at && (
            <time dateTime={commit.authored_at} title={absoluteTime(commit.authored_at)}>
              {relativeTime(commit.authored_at)}
            </time>
          )}
        </span>
      </button>
    </li>
  );
}
