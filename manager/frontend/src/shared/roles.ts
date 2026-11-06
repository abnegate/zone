import type { OrgRole, WorkspaceRole } from '../types';

export const ORGANIZATION_RANK: Record<OrgRole, number> = { member: 0, admin: 1, owner: 2 };

export const WORKSPACE_RANK: Record<WorkspaceRole, number> = {
  viewer: 0,
  member: 1,
  admin: 2,
  owner: 3,
};

export function meetsOrganizationRole(role: OrgRole, required: OrgRole): boolean {
  return ORGANIZATION_RANK[role] >= ORGANIZATION_RANK[required];
}

export function meetsWorkspaceRole(role: WorkspaceRole, required: WorkspaceRole): boolean {
  return WORKSPACE_RANK[role] >= WORKSPACE_RANK[required];
}
