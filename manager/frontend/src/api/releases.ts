import { GitHubApi, REFRESH } from '../features/github/GitHubApi';
import { ReleaseRefusal } from '../features/releases/ReleaseRefusal';
import {
  CreatedResponseSchema,
  CreateReleaseRequestSchema,
  OptionsResponseSchema,
  PipelineResponseSchema,
  PreviewRequestSchema,
  PreviewResponseSchema,
  RefusalSchema,
  ReleasesResponseSchema,
} from '../features/releases/schemas';
import type {
  Created,
  CreateReleaseRequest,
  OptionsResponse,
  PipelineResponse,
  Preview,
  PreviewRequest,
  RefreshQuery,
  ReleasesQuery,
  ReleasesResponse,
} from '../features/releases/types';
import { parse } from '../validation';
import type { ApiError } from './ApiError';
import { API_BASE } from './client';

function search(parameters: Record<string, string | undefined>): string {
  const defined = Object.entries(parameters).filter(
    (entry): entry is [string, string] => entry[1] !== undefined
  );
  return defined.length > 0 ? `?${new URLSearchParams(defined)}` : '';
}

function refreshing(query: RefreshQuery): string | undefined {
  return query.refresh ? REFRESH : undefined;
}

function releasesUrl(workspaceId: string): string {
  return `${API_BASE}/api/workspaces/${encodeURIComponent(workspaceId)}/releases`;
}

class ReleasesApi extends GitHubApi {
  protected refusal(body: unknown): ApiError | null {
    const refusal = RefusalSchema.safeParse(body);
    return refusal.success ? new ReleaseRefusal(refusal.data) : null;
  }

  async list(
    workspaceId: string,
    query: ReleasesQuery = {},
    signal?: AbortSignal
  ): Promise<ReleasesResponse> {
    const parameters = search({
      days: query.days === undefined ? undefined : String(query.days),
      refresh: refreshing(query),
    });
    const data = await this.send(
      `${releasesUrl(workspaceId)}${parameters}`,
      { signal },
      'Failed to fetch releases'
    );
    return parse(ReleasesResponseSchema, data);
  }

  async options(
    workspaceId: string,
    query: RefreshQuery = {},
    signal?: AbortSignal
  ): Promise<OptionsResponse> {
    const parameters = search({ refresh: refreshing(query) });
    const data = await this.send(
      `${releasesUrl(workspaceId)}/options${parameters}`,
      { signal },
      'Failed to fetch release options'
    );
    return parse(OptionsResponseSchema, data);
  }

  async pipeline(
    workspaceId: string,
    sourceId: string,
    releaseId: number,
    query: RefreshQuery = {},
    signal?: AbortSignal
  ): Promise<PipelineResponse> {
    const parameters = search({ refresh: refreshing(query) });
    const path = `${encodeURIComponent(sourceId)}/${encodeURIComponent(String(releaseId))}`;
    const data = await this.send(
      `${releasesUrl(workspaceId)}/${path}/pipeline${parameters}`,
      { signal },
      'Failed to fetch release pipeline'
    );
    return parse(PipelineResponseSchema, data);
  }

  async preview(
    workspaceId: string,
    request: PreviewRequest,
    signal?: AbortSignal
  ): Promise<Preview> {
    const data = await this.send(
      `${releasesUrl(workspaceId)}/preview`,
      { method: 'POST', body: JSON.stringify(parse(PreviewRequestSchema, request)), signal },
      'Failed to preview release'
    );
    return parse(PreviewResponseSchema, data).preview;
  }

  async create(
    workspaceId: string,
    request: CreateReleaseRequest,
    signal?: AbortSignal
  ): Promise<Created> {
    const data = await this.send(
      releasesUrl(workspaceId),
      { method: 'POST', body: JSON.stringify(parse(CreateReleaseRequestSchema, request)), signal },
      'Failed to create release'
    );
    return parse(CreatedResponseSchema, data).release;
  }
}

export const releasesApi = new ReleasesApi();
