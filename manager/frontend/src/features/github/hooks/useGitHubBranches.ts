import { skipToken, useQuery } from '@tanstack/react-query';
import { githubApi } from '../../../api/github';
import type { GitHubBranch, GitHubRepository } from '../types';
import { githubQueryKeys } from './githubQueryKeys';

const NO_BRANCHES: GitHubBranch[] = [];

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
        : () =>
            githubApi.listBranches(
              organizationId,
              repository.installation_id,
              repository.owner,
              repository.name,
              1
            ),
  });

  return {
    branches: data?.branches ?? NO_BRANCHES,
    loading: isLoading,
    error: error ? error.message : null,
  };
}
