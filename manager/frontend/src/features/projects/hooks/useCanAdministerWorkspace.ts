import type { WorkspaceRole } from '../../auth/types';
import { useWorkspaceRole } from './useWorkspaceRole';

const ADMINISTERING_ROLES: ReadonlySet<WorkspaceRole> = new Set(['owner', 'admin']);

/**
 * Whether the signed-in user administers the workspace. While the role is
 * loading the answer is no; if it cannot be read the answer is yes, so an
 * admin is never locked out and the server's refusal speaks for itself.
 */
export function useCanAdministerWorkspace(workspaceId: string | undefined): boolean {
  const { data: role, isError } = useWorkspaceRole(workspaceId);
  if (isError) return true;
  return role != null && ADMINISTERING_ROLES.has(role);
}
