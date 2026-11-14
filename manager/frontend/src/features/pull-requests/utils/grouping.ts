import type { Tint } from '../../github';
import type { PullRequest, Readiness } from '../types';

export const READINESS_ORDER: readonly Readiness[] = ['ready', 'in_progress', 'not_ready'];

export const READINESS_LABELS: Readonly<Record<Readiness, string>> = {
  ready: 'Ready',
  in_progress: 'In progress',
  not_ready: 'Not ready',
};

export const READINESS_TINTS: Readonly<Record<Readiness, Tint>> = {
  ready: 'success',
  in_progress: 'info',
  not_ready: 'warning',
};

export interface PullRequestGroup {
  readiness: Readiness;
  label: string;
  tint: Tint;
  pullRequests: readonly PullRequest[];
}

export interface PullRequestFilter {
  readiness?: Readiness | null;
  repository?: string | null;
  author?: string | null;
}

export function groupPullRequests(pullRequests: readonly PullRequest[]): PullRequestGroup[] {
  const buckets = new Map<Readiness, PullRequest[]>(
    READINESS_ORDER.map((readiness) => [readiness, []])
  );
  for (const pullRequest of pullRequests) {
    buckets.get(pullRequest.readiness)?.push(pullRequest);
  }
  return READINESS_ORDER.flatMap((readiness) => {
    const grouped = buckets.get(readiness) ?? [];
    if (grouped.length === 0) return [];
    return [
      {
        readiness,
        label: READINESS_LABELS[readiness],
        tint: READINESS_TINTS[readiness],
        pullRequests: grouped,
      },
    ];
  });
}

export function filterPullRequests(
  pullRequests: readonly PullRequest[],
  filter: PullRequestFilter
): PullRequest[] {
  return pullRequests.filter(
    (pullRequest) =>
      (!filter.readiness || pullRequest.readiness === filter.readiness) &&
      (!filter.repository || pullRequest.repository === filter.repository) &&
      (!filter.author || pullRequest.author === filter.author)
  );
}

function distinct(values: Iterable<string | null>): string[] {
  const seen = new Set<string>();
  for (const value of values) {
    if (value) seen.add(value);
  }
  return [...seen].sort((left, right) => left.localeCompare(right));
}

export function repositoriesOf(pullRequests: readonly PullRequest[]): string[] {
  return distinct(pullRequests.map((pullRequest) => pullRequest.repository));
}

export function authorsOf(pullRequests: readonly PullRequest[]): string[] {
  return distinct(pullRequests.map((pullRequest) => pullRequest.author));
}
