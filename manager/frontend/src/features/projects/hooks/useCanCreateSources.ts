import type { WorkspaceRole } from '../../auth/types';
import { useWorkspaceRole } from './useWorkspaceRole';

const WRITING_ROLES: ReadonlySet<WorkspaceRole> = new Set(['owner', 'admin', 'member']);

/**
 * Whether the signed-in user may add sources to the workspace: the server lets
 * members and above write and refuses viewers. While the role is loading the
 * answer is no; if it cannot be read the answer is yes, and the server's
 * refusal speaks for itself.
 */
export function useCanCreateSources(workspaceId: string | undefined): boolean {
  const { data: role, isError } = useWorkspaceRole(workspaceId);
  if (isError) return true;
  return role != null && WRITING_ROLES.has(role);
}
