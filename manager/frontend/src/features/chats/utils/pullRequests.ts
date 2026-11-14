import type {
  ChatPullRequest,
  PullRequestConnection,
  PullRequestStatus,
  PullRequestThreads,
} from '../types';

export const UNCREDENTIALED_HINT = 'No connected GitHub source holds a credential for it.';
export const DISCONNECTED_HINT =
  'The GitHub connection it was reached through was removed or suspended. Reconnect GitHub.';
export const UNAVAILABLE_HINT = 'GitHub is temporarily unreachable. Try again in a moment.';
export const READ_ONLY_HINT = 'Needs write access to this workspace.';
export const AUTO_MERGE_HINT = 'GitHub merges it once its checks and reviews pass.';
export const AUTO_FIX_HINT =
  'Zone starts a run to fix it whenever GitHub reports failing checks or a conflict with its base branch.';
export const AUTO_FIXING_HINT =
  'A run is working on its branch now, and pushes what it changes when it finishes.';

export type PullRequestKind =
  | 'merged'
  | 'closed'
  | 'draft'
  | 'conflicted'
  | 'failing'
  | 'pending'
  | 'passing'
  | 'open'
  | 'unknown';

export const PULL_REQUEST_KIND_LABELS: Record<PullRequestKind, string> = {
  merged: 'Merged',
  closed: 'Closed',
  draft: 'Draft',
  conflicted: 'Has merge conflicts',
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
  // A conflicted branch cannot merge whatever its checks say, so it never takes a check mark.
  if (status.mergeable === 'conflicted') return 'conflicted';
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

export function threadsLabel({ open, resolved }: PullRequestThreads): string {
  const noun = open === 1 ? 'review thread' : 'review threads';
  return `${open} open ${noun}, ${resolved} resolved`;
}

export function pullRequestTitle(pullRequest: ChatPullRequest): string {
  return pullRequest.title?.trim() || `Pull request #${pullRequest.number}`;
}

export function pullRequestName(pullRequest: ChatPullRequest): string {
  return `${pullRequest.repo} #${pullRequest.number}`;
}

const CONNECTION_HINTS: Record<Exclude<PullRequestConnection, 'credentialed'>, string> = {
  uncredentialed: UNCREDENTIALED_HINT,
  disconnected: DISCONNECTED_HINT,
  unavailable: UNAVAILABLE_HINT,
};

/** Why GitHub cannot be asked about the pull request, or nothing when it can. */
export function connectionHint({ credentialed, connection }: ChatPullRequest): string | undefined {
  if (credentialed) {
    return undefined;
  }
  return connection === 'credentialed' ? UNCREDENTIALED_HINT : CONNECTION_HINTS[connection];
}
