import { skipToken, useQuery } from '@tanstack/react-query';
import { ApiError } from '../../../api/ApiError';
import { githubApi } from '../../../api/github';
import { HttpStatus } from '../../../api/HttpStatus';
import type { GitHubBranch, GitHubRepository } from '../types';
import { githubQueryKeys } from './githubQueryKeys';

const NO_BRANCHES: GitHubBranch[] = [];
const FIRST_PAGE = 1;

async function listEveryBranch(
  organizationId: string,
  repository: GitHubRepository
): Promise<GitHubBranch[]> {
  const branches: GitHubBranch[] = [];
  let page: number | null = FIRST_PAGE;
  while (page !== null) {
    const response = await githubApi.listBranches(
      organizationId,
      repository.installation_id,
      repository.owner,
      repository.name,
      page
    );
    branches.push(...response.branches);
    page = response.next_page !== null && response.next_page > page ? response.next_page : null;
  }
  return branches;
}

function isNotFound(error: Error): boolean {
  return error instanceof ApiError && error.status === HttpStatus.NotFound;
}

export function useGitHubBranches(
  organizationId: string | null,
  repository: GitHubRepository | null
) {
  const { data, isLoading, error } = useQuery({
    queryKey: githubQueryKeys.branches(
      organizationId,
      repository?.installation_id ?? '',
      repository?.owner ?? '',
      repository?.name ?? ''
    ),
    queryFn:
      organizationId === null || repository === null
        ? skipToken
        : () => listEveryBranch(organizationId, repository),
  });

  return {
    branches: data ?? NO_BRANCHES,
    loading: isLoading,
    failed: error !== null && !isNotFound(error),
  };
}
