import type { WorkspaceRole } from '../../auth/types';
import { useWorkspaceRole } from './useWorkspaceRole';

const WRITING_ROLES: ReadonlySet<WorkspaceRole> = new Set(['owner', 'admin', 'member']);

/**
 * Whether the signed-in user may change the workspace's projects. While the role
 * is loading the answer is no; if it cannot be read the answer is yes, so a member
 * is never locked out and the server's refusal speaks for itself.
 */
export function useCanWriteWorkspace(workspaceId: string | undefined): boolean {
  const { data: role, isError } = useWorkspaceRole(workspaceId);
  if (isError) return true;
  return role != null && WRITING_ROLES.has(role);
}
