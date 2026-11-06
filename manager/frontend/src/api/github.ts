import { REPOSITORY_PAGE_SIZE } from '../features/github/constants';
import {
  GitHubAppRegistrationSchema,
  GitHubBranchesResponseSchema,
  GitHubConnectionSchema,
  GitHubInstallationUrlSchema,
  GitHubRepositoriesResponseSchema,
} from '../features/github/schemas';
import type {
  GitHubAppRegistration,
  GitHubBranchesResponse,
  GitHubConnection,
  GitHubRepositoriesResponse,
} from '../features/github/types';
import { parse } from '../validation';
import { ApiError } from './ApiError';
import { API_BASE, client } from './client';

export interface GitHubRepositorySearch {
  query?: string;
  page?: number;
  perPage?: number;
  refresh?: boolean;
}

function githubUrl(organizationId: string): string {
  return `${API_BASE}/api/organizations/${encodeURIComponent(organizationId)}/github`;
}

async function send(url: string, init: RequestInit, action: string): Promise<Response> {
  const response = await fetch(url, { ...init, headers: client.getHeaders() });
  if (!response.ok) {
    throw await ApiError.from(response, action);
  }
  return response;
}

function repositorySearchParameters(search: GitHubRepositorySearch): URLSearchParams {
  const parameters = new URLSearchParams();
  if (search.query) {
    parameters.append('query', search.query);
  }
  parameters.append('page', String(search.page ?? 1));
  parameters.append('per_page', String(search.perPage ?? REPOSITORY_PAGE_SIZE));
  if (search.refresh) {
    parameters.append('refresh', 'true');
  }
  return parameters;
}

export const githubApi = {
  async getConnection(organizationId: string): Promise<GitHubConnection> {
    const response = await send(
      githubUrl(organizationId),
      { method: 'GET' },
      'Failed to load the GitHub connection'
    );
    return parse(GitHubConnectionSchema, await response.json());
  },

  async registerApp(organizationId: string, owner: string | null): Promise<GitHubAppRegistration> {
    const response = await send(
      `${githubUrl(organizationId)}/apps`,
      { method: 'POST', body: JSON.stringify({ owner }) },
      'Failed to connect GitHub'
    );
    return parse(GitHubAppRegistrationSchema, await response.json());
  },

  async createInstallation(organizationId: string, appId: string): Promise<string> {
    const response = await send(
      `${githubUrl(organizationId)}/apps/${encodeURIComponent(appId)}/installations`,
      { method: 'POST' },
      'Failed to start the GitHub installation'
    );
    return parse(GitHubInstallationUrlSchema, await response.json()).url;
  },

  async syncInstallations(organizationId: string): Promise<void> {
    await send(
      `${githubUrl(organizationId)}/installations/sync`,
      { method: 'POST' },
      'Failed to refresh GitHub accounts'
    );
  },

  async removeInstallation(
    organizationId: string,
    installationId: string,
    uninstall: boolean
  ): Promise<void> {
    await send(
      `${githubUrl(organizationId)}/installations/${encodeURIComponent(installationId)}?uninstall=${uninstall}`,
      { method: 'DELETE' },
      'Failed to remove the GitHub account'
    );
  },

  async deleteApp(organizationId: string, appId: string): Promise<void> {
    await send(
      `${githubUrl(organizationId)}/apps/${encodeURIComponent(appId)}`,
      { method: 'DELETE' },
      'Failed to remove the GitHub App'
    );
  },

  async listRepositories(
    organizationId: string,
    search: GitHubRepositorySearch = {}
  ): Promise<GitHubRepositoriesResponse> {
    const response = await send(
      `${githubUrl(organizationId)}/repositories?${repositorySearchParameters(search)}`,
      { method: 'GET' },
      'Failed to load repositories'
    );
    return parse(GitHubRepositoriesResponseSchema, await response.json());
  },

  async listBranches(
    organizationId: string,
    installationId: string,
    owner: string,
    repository: string,
    page = 1
  ): Promise<GitHubBranchesResponse> {
    const path = ['installations', installationId, 'repositories', owner, repository, 'branches']
      .map((segment) => encodeURIComponent(segment))
      .join('/');
    const response = await send(
      `${githubUrl(organizationId)}/${path}?page=${page}`,
      { method: 'GET' },
      'Failed to load branches'
    );
    return parse(GitHubBranchesResponseSchema, await response.json());
  },
};
