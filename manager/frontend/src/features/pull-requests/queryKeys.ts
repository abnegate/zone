import type { InvalidateQueryFilters } from '@tanstack/react-query';
import type { PullRequestRevision } from './types';

export const PULL_REQUESTS_KEY = 'pull-requests';

export const PullRequestQuery = {
  List: 'list',
  Detail: 'detail',
  Diff: 'diff',
  Commits: 'commits',
  Commit: 'commit',
  Log: 'log',
} as const;

export type PullRequestQuery = (typeof PullRequestQuery)[keyof typeof PullRequestQuery];

const LISTINGS: readonly unknown[] = [PullRequestQuery.List, PullRequestQuery.Detail];

const QUERY_POSITION = 2;

type Workspace = string | undefined;

type Revision = PullRequestRevision | null;

function scoped(workspaceId: Workspace, query: PullRequestQuery, revision: Revision) {
  return [
    PULL_REQUESTS_KEY,
    workspaceId,
    query,
    revision?.source_id ?? null,
    revision?.number ?? null,
    revision?.head ?? null,
  ] as const;
}

export const pullRequestKeys = {
  workspace: (workspaceId: Workspace) => [PULL_REQUESTS_KEY, workspaceId] as const,
  list: (workspaceId: Workspace) =>
    [PULL_REQUESTS_KEY, workspaceId, PullRequestQuery.List] as const,
  detail: (workspaceId: Workspace, revision: Revision) =>
    scoped(workspaceId, PullRequestQuery.Detail, revision),
  diff: (workspaceId: Workspace, revision: Revision) =>
    scoped(workspaceId, PullRequestQuery.Diff, revision),
  commits: (workspaceId: Workspace, revision: Revision) =>
    scoped(workspaceId, PullRequestQuery.Commits, revision),
  commit: (workspaceId: Workspace, revision: Revision, sha: string | null) =>
    [...scoped(workspaceId, PullRequestQuery.Commit, revision), sha] as const,
  log: (workspaceId: Workspace, revision: Revision, jobId: number | null) =>
    [...scoped(workspaceId, PullRequestQuery.Log, revision), jobId] as const,
};

export function pullRequestListings(workspaceId: Workspace): InvalidateQueryFilters {
  return {
    queryKey: pullRequestKeys.workspace(workspaceId),
    predicate: (query) => LISTINGS.includes(query.queryKey[QUERY_POSITION]),
  };
}
