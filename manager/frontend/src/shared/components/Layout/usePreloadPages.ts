import { useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';
import { useAuth } from '../../../features/auth/context';
import { preloadPullRequests } from '../../../features/pull-requests/hooks/preload';
import { preloadReleases } from '../../../features/releases/hooks/preload';
import { useWorkspace } from '../../context/WorkspaceContext';
import { PERMISSIONS } from '../../types/permissions';

export function usePreloadPages(): void {
  const queryClient = useQueryClient();
  const { hasPermission } = useAuth();
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const readsPullRequests = hasPermission(PERMISSIONS.PULL_REQUESTS.READ);
  const readsReleases = hasPermission(PERMISSIONS.RELEASES.READ);

  useEffect(() => {
    if (!workspaceId) return;
    if (readsPullRequests) void preloadPullRequests(queryClient, workspaceId);
    if (readsReleases) void preloadReleases(queryClient, workspaceId);
  }, [queryClient, workspaceId, readsPullRequests, readsReleases]);
}
