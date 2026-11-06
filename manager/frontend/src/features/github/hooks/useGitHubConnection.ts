import { skipToken, useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { githubApi } from '../../../api/github';
import type { GitHubApp, GitHubAppRegistration, GitHubInstallation } from '../types';
import { githubQueryKeys } from './githubQueryKeys';

const NO_APPS: GitHubApp[] = [];
const NO_INSTALLATIONS: GitHubInstallation[] = [];

function requireOrganization(organizationId: string | null): string {
  if (organizationId === null) {
    throw new Error('An organization is required to manage GitHub');
  }
  return organizationId;
}

export function useGitHubConnection(organizationId: string | null) {
  const queryClient = useQueryClient();

  const { data, isLoading, error, refetch } = useQuery({
    queryKey: githubQueryKeys.connection(organizationId),
    queryFn: organizationId === null ? skipToken : () => githubApi.getConnection(organizationId),
  });

  const invalidate = useCallback(
    () => queryClient.invalidateQueries({ queryKey: githubQueryKeys.organization(organizationId) }),
    [queryClient, organizationId]
  );

  const refresh = useCallback(async (): Promise<void> => {
    if (organizationId === null) {
      return;
    }
    await refetch();
  }, [organizationId, refetch]);

  const registerApp = useCallback(
    (owner: string | null): Promise<GitHubAppRegistration> =>
      githubApi.registerApp(requireOrganization(organizationId), owner),
    [organizationId]
  );

  const createInstallation = useCallback(
    (appId: string): Promise<string> =>
      githubApi.createInstallation(requireOrganization(organizationId), appId),
    [organizationId]
  );

  const syncInstallations = useCallback(async (): Promise<void> => {
    await githubApi.syncInstallations(requireOrganization(organizationId));
    await invalidate();
  }, [organizationId, invalidate]);

  const removeInstallation = useCallback(
    async (installationId: string, uninstall: boolean): Promise<void> => {
      await githubApi.removeInstallation(
        requireOrganization(organizationId),
        installationId,
        uninstall
      );
      await invalidate();
    },
    [organizationId, invalidate]
  );

  const deleteApp = useCallback(
    async (appId: string): Promise<void> => {
      await githubApi.deleteApp(requireOrganization(organizationId), appId);
      await invalidate();
    },
    [organizationId, invalidate]
  );

  return {
    apps: data?.apps ?? NO_APPS,
    installations: data?.installations ?? NO_INSTALLATIONS,
    loading: isLoading,
    error: error ? error.message : null,
    refresh,
    registerApp,
    createInstallation,
    syncInstallations,
    removeInstallation,
    deleteApp,
  };
}
