import { meetsOrganizationRole, meetsWorkspaceRole } from '../../../shared/roles';
import type { OrgRole, WorkspaceRole } from '../../../types';
import {
  type ORGANIZATION_PERMISSIONS,
  Permission,
  type WORKSPACE_PERMISSIONS,
} from '../../enterprise/roles/schemas';

export type OrganizationPermission = (typeof ORGANIZATION_PERMISSIONS)[number];

export type WorkspacePermission = (typeof WORKSPACE_PERMISSIONS)[number];

export const ORGANIZATION_FLOOR: Readonly<Record<OrganizationPermission, OrgRole>> = {
  [Permission.OrganizationRead]: 'member',
  [Permission.OrganizationUpdate]: 'admin',
  [Permission.OrganizationDelete]: 'owner',
  [Permission.MembersCreate]: 'admin',
  [Permission.MembersUpdate]: 'admin',
  [Permission.MembersDelete]: 'admin',
  [Permission.AdministratorsUpdate]: 'owner',
  [Permission.InvitationsCreate]: 'admin',
  [Permission.InvitationsRead]: 'admin',
  [Permission.InvitationsDelete]: 'admin',
  [Permission.WorkspacesCreate]: 'admin',
  [Permission.DevicesRead]: 'admin',
  [Permission.DevicesUpdate]: 'admin',
  [Permission.GitHubUpdate]: 'admin',
  [Permission.McpServersUpdate]: 'admin',
  [Permission.AiSettingsRead]: 'admin',
  [Permission.AiSettingsUpdate]: 'admin',
  [Permission.AgentLoginsUpdate]: 'admin',
  [Permission.PoliciesUpdate]: 'admin',
  [Permission.AuditLogsRead]: 'admin',
  [Permission.AuditUpdate]: 'admin',
  [Permission.SpendRead]: 'admin',
  [Permission.SpendUpdate]: 'admin',
  [Permission.RolesUpdate]: 'admin',
};

export const WORKSPACE_FLOOR: Readonly<Record<WorkspacePermission, WorkspaceRole>> = {
  [Permission.WorkspaceRead]: 'viewer',
  [Permission.WorkspaceWrite]: 'member',
  [Permission.WorkspaceUpdate]: 'admin',
  [Permission.WorkspaceDelete]: 'owner',
  [Permission.WorkspaceMembersCreate]: 'admin',
  [Permission.WorkspaceMembersUpdate]: 'admin',
  [Permission.WorkspaceMembersDelete]: 'admin',
  [Permission.WorkspaceAdministratorsUpdate]: 'owner',
  [Permission.WorkspaceAiSettingsUpdate]: 'admin',
  [Permission.WorkspaceMcpServersUpdate]: 'admin',
  [Permission.WorkspacePoliciesUpdate]: 'admin',
  [Permission.ProjectSyncsUpdate]: 'admin',
  [Permission.PullRequestsOverride]: 'admin',
};

export const WORKSPACE_EDIT_PERMISSION = {
  theme: Permission.WorkspaceWrite,
  ai: Permission.WorkspaceAiSettingsUpdate,
  folders: Permission.WorkspaceWrite,
  mcp: Permission.WorkspaceMcpServersUpdate,
  sources: Permission.WorkspaceWrite,
} as const satisfies Record<string, WorkspacePermission>;

export function allowsOrganization(role: OrgRole | undefined, required: OrgRole): boolean {
  return role === undefined || meetsOrganizationRole(role, required);
}

export function allowsWorkspace(role: WorkspaceRole | undefined, required: WorkspaceRole): boolean {
  return role === undefined || meetsWorkspaceRole(role, required);
}

export function grantsOrganization(
  permissions: readonly string[] | undefined,
  role: OrgRole | undefined,
  name: OrganizationPermission
): boolean {
  return permissions === undefined
    ? allowsOrganization(role, ORGANIZATION_FLOOR[name])
    : permissions.includes(name);
}

export function grantsWorkspace(
  permissions: readonly string[] | undefined,
  role: WorkspaceRole | undefined,
  name: WorkspacePermission
): boolean {
  return permissions === undefined
    ? allowsWorkspace(role, WORKSPACE_FLOOR[name])
    : permissions.includes(name);
}
