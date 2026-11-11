import { meetsOrganizationRole, meetsWorkspaceRole } from '../../../shared/roles';
import type { OrgRole, WorkspaceRole } from '../../../types';

export const WORKSPACE_EDIT_ROLE = {
  theme: 'member',
  ai: 'admin',
  folders: 'member',
  mcp: 'admin',
} as const satisfies Record<string, WorkspaceRole>;

export const ORGANIZATION_ADMIN_ROLE: OrgRole = 'admin';

export function allowsOrganization(role: OrgRole | undefined, required: OrgRole): boolean {
  return role === undefined || meetsOrganizationRole(role, required);
}

export function allowsWorkspace(role: WorkspaceRole | undefined, required: WorkspaceRole): boolean {
  return role === undefined || meetsWorkspaceRole(role, required);
}
