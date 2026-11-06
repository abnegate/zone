import type { WorkspaceRole } from '../../auth/types';
import { useWorkspaceRole } from './useWorkspaceRole';

const WRITING_ROLES: ReadonlySet<WorkspaceRole> = new Set(['owner', 'admin', 'member']);

/** Whether the signed-in user may change the workspace's projects; no until their role is known. */
export function useCanWriteWorkspace(workspaceId: string | undefined): boolean {
  const { data: role } = useWorkspaceRole(workspaceId);
  return role != null && WRITING_ROLES.has(role);
}
