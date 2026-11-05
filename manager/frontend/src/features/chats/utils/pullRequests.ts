import type { ChatPullRequest, PullRequestStatus } from '../types';

export const UNCREDENTIALED_HINT = 'No connected GitHub source holds a credential for it.';
export const READ_ONLY_HINT = 'Needs write access to this workspace.';
export const AUTO_MERGE_HINT = 'GitHub merges it once its checks and reviews pass.';

export type PullRequestKind =
  | 'merged'
  | 'closed'
  | 'draft'
  | 'failing'
  | 'pending'
  | 'passing'
  | 'open'
  | 'unknown';

export const PULL_REQUEST_KIND_LABELS: Record<PullRequestKind, string> = {
  merged: 'Merged',
  closed: 'Closed',
  draft: 'Draft',
  failing: 'Checks failing',
  pending: 'Checks running',
  passing: 'Checks passing',
  open: 'Open',
  unknown: 'Status unavailable',
};

export function pullRequestKind(pullRequest: ChatPullRequest): PullRequestKind {
  const status = pullRequest.status;
  if (!status) return 'unknown';
  if (status.state === 'merged') return 'merged';
  if (status.state === 'closed') return 'closed';
  if (status.draft) return 'draft';
  if (status.checks === 'failing') return 'failing';
  if (status.checks === 'pending') return 'pending';
  if (status.checks === 'passing') return 'passing';
  return 'open';
}

export type MergeSignal = 'ready' | 'waiting' | 'conflicted';

export interface MergeReading {
  signal: MergeSignal;
  label: string;
}

/** Whether an open pull request can merge as it stands, for its status dot. */
export function mergeReading(status: PullRequestStatus | null): MergeReading | null {
  if (status?.state !== 'open' || status.draft) return null;
  switch (status.mergeable) {
    case 'clean':
      return { signal: 'ready', label: 'Ready to merge' };
    case 'blocked':
      return { signal: 'waiting', label: 'Merging is blocked' };
    case 'behind':
      return { signal: 'waiting', label: 'Behind its base branch' };
    case 'unstable':
      return { signal: 'waiting', label: 'Some checks are not passing' };
    case 'conflicted':
      return { signal: 'conflicted', label: 'Has merge conflicts' };
    default:
      return null;
  }
}

export function pullRequestTitle(pullRequest: ChatPullRequest): string {
  return pullRequest.title?.trim() || `Pull request #${pullRequest.number}`;
}

export function pullRequestName(pullRequest: ChatPullRequest): string {
  return `${pullRequest.repo} #${pullRequest.number}`;
}
