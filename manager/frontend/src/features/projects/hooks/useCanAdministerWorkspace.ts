import { useQuery } from '@tanstack/react-query';
import { client } from '../../../api/client';
import { useAuth } from '../../auth';
import type { WorkspaceRole } from '../../auth/types';

const ADMINISTERING_ROLES: ReadonlySet<WorkspaceRole> = new Set(['owner', 'admin']);

/**
 * Whether the signed-in user administers the workspace. While the role is
 * loading the answer is no; if it cannot be read the answer is yes, so an
 * admin is never locked out and the server's refusal speaks for itself.
 */
export function useCanAdministerWorkspace(workspaceId: string | undefined): boolean {
  const { isAuthenticated, user } = useAuth();
  const { data: role, isError } = useQuery({
    queryKey: ['workspaceRole', workspaceId, user?.id],
    queryFn: async () => {
      const { members } = await client.getWorkspaceMembers(workspaceId as string);
      return members.find((member) => member.user_id === user?.id)?.role ?? null;
    },
    enabled: isAuthenticated && !!workspaceId && !!user?.id,
  });
  if (isError) return true;
  return role != null && ADMINISTERING_ROLES.has(role);
}
