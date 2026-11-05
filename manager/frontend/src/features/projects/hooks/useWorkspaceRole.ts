import { type UseQueryResult, useQuery } from '@tanstack/react-query';
import { client } from '../../../api/client';
import { useAuth } from '../../auth';
import type { WorkspaceRole } from '../../auth/types';

export function useWorkspaceRole(
  workspaceId: string | undefined
): UseQueryResult<WorkspaceRole | null> {
  const { isAuthenticated, user } = useAuth();
  return useQuery({
    queryKey: ['workspaceRole', workspaceId, user?.id],
    queryFn: async () => {
      const { members } = await client.getWorkspaceMembers(workspaceId as string);
      return members.find((member) => member.user_id === user?.id)?.role ?? null;
    },
    enabled: isAuthenticated && !!workspaceId && !!user?.id,
  });
}
