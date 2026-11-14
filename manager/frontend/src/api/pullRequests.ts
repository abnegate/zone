import { GitHubApi, REFRESH } from '../features/github/GitHubApi';
import { PullRequestRefusal } from '../features/pull-requests/PullRequestRefusal';
import {
  CheckLogResponseSchema,
  CommitsResponseSchema,
  DiffResponseSchema,
  MergedResponseSchema,
  MergeRequestSchema,
  PullRequestDetailSchema,
  PullRequestsResponseSchema,
  RefusalSchema,
} from '../features/pull-requests/schemas';
import type {
  CheckLog,
  CommitList,
  Diff,
  Merged,
  MergeRequest,
  PullRequestDetail,
  PullRequestReference,
  PullRequestsResponse,
} from '../features/pull-requests/types';
import { parse } from '../validation';
import type { ApiError } from './ApiError';
import { API_BASE } from './client';

function collectionUrl(workspaceId: string): string {
  return `${API_BASE}/api/workspaces/${encodeURIComponent(workspaceId)}/pull-requests`;
}

function pullRequestUrl(workspaceId: string, reference: PullRequestReference): string {
  return `${collectionUrl(workspaceId)}/${encodeURIComponent(reference.source_id)}/${encodeURIComponent(reference.number)}`;
}

function headQuery(head: string): string {
  return `?${new URLSearchParams({ head })}`;
}

class PullRequestsApi extends GitHubApi {
  protected refusal(body: unknown): ApiError | null {
    const refusal = RefusalSchema.safeParse(body);
    return refusal.success ? new PullRequestRefusal(refusal.data) : null;
  }

  async list(
    workspaceId: string,
    refresh = false,
    signal?: AbortSignal
  ): Promise<PullRequestsResponse> {
    const query = refresh ? `?${new URLSearchParams({ refresh: REFRESH })}` : '';
    const data = await this.send(
      `${collectionUrl(workspaceId)}${query}`,
      { signal },
      'Failed to fetch pull requests'
    );
    return parse(PullRequestsResponseSchema, data);
  }

  async detail(
    workspaceId: string,
    reference: PullRequestReference,
    signal?: AbortSignal
  ): Promise<PullRequestDetail> {
    const data = await this.send(
      pullRequestUrl(workspaceId, reference),
      { signal },
      'Failed to fetch pull request'
    );
    return parse(PullRequestDetailSchema, data);
  }

  async diff(
    workspaceId: string,
    reference: PullRequestReference,
    head: string,
    signal?: AbortSignal
  ): Promise<Diff> {
    const data = await this.send(
      `${pullRequestUrl(workspaceId, reference)}/diff${headQuery(head)}`,
      { signal },
      'Failed to fetch pull request diff'
    );
    return parse(DiffResponseSchema, data).diff;
  }

  async commits(
    workspaceId: string,
    reference: PullRequestReference,
    head: string,
    signal?: AbortSignal
  ): Promise<CommitList> {
    const data = await this.send(
      `${pullRequestUrl(workspaceId, reference)}/commits${headQuery(head)}`,
      { signal },
      'Failed to fetch pull request commits'
    );
    return parse(CommitsResponseSchema, data).commits;
  }

  async commitDiff(
    workspaceId: string,
    reference: PullRequestReference,
    sha: string,
    head: string,
    signal?: AbortSignal
  ): Promise<Diff> {
    const data = await this.send(
      `${pullRequestUrl(workspaceId, reference)}/commits/${encodeURIComponent(sha)}${headQuery(head)}`,
      { signal },
      'Failed to fetch commit diff'
    );
    return parse(DiffResponseSchema, data).diff;
  }

  async jobLog(
    workspaceId: string,
    reference: PullRequestReference,
    jobId: number,
    head: string,
    signal?: AbortSignal
  ): Promise<CheckLog> {
    const data = await this.send(
      `${pullRequestUrl(workspaceId, reference)}/jobs/${encodeURIComponent(jobId)}/log${headQuery(head)}`,
      { signal },
      'Failed to fetch check log'
    );
    return parse(CheckLogResponseSchema, data).log;
  }

  async merge(
    workspaceId: string,
    reference: PullRequestReference,
    request: MergeRequest
  ): Promise<Merged> {
    const data = await this.send(
      `${pullRequestUrl(workspaceId, reference)}/merge`,
      { method: 'POST', body: JSON.stringify(parse(MergeRequestSchema, request)) },
      'Failed to merge pull request'
    );
    return parse(MergedResponseSchema, data).merged;
  }
}

export const pullRequestsApi = new PullRequestsApi();
