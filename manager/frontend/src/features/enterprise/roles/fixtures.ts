import { Edition, LicenseStatus } from '../schemas';
import {
  ORGANIZATION_PERMISSIONS,
  Permission,
  type PermissionName,
  RoleScope,
  WORKSPACE_PERMISSIONS,
} from './schemas';
import type { CustomRole, PermissionDefinition, RolesResponse } from './types';

export const ORGANIZATION_ID = '9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d';

export const CREATED_AT = '2026-11-15T10:30:00Z';

export const UPDATED_AT = '2026-11-18T08:00:00Z';

const RESERVED: ReadonlySet<PermissionName> = new Set([
  Permission.OrganizationRead,
  Permission.OrganizationDelete,
  Permission.AdministratorsUpdate,
  Permission.RolesUpdate,
  Permission.ScimUpdate,
  Permission.WorkspaceRead,
  Permission.WorkspaceDelete,
  Permission.WorkspaceAdministratorsUpdate,
]);

function definitions(names: readonly PermissionName[], scope: RoleScope): PermissionDefinition[] {
  return names.map((name) => ({ name, scope, composable: !RESERVED.has(name) }));
}

export const CATALOGUE: PermissionDefinition[] = [
  ...definitions(ORGANIZATION_PERMISSIONS, RoleScope.Organization),
  ...definitions(WORKSPACE_PERMISSIONS, RoleScope.Workspace),
];

export const RELEASE_MANAGER: CustomRole = {
  id: 'b3c1d2e4-0000-4000-8000-000000000001',
  organization_id: ORGANIZATION_ID,
  name: 'Release manager',
  description: 'Merges and syncs, nothing else.',
  scope: RoleScope.Workspace,
  permissions: [
    Permission.WorkspaceRead,
    Permission.WorkspaceWrite,
    Permission.ProjectSyncsUpdate,
    Permission.PullRequestsOverride,
  ],
  members: 3,
  created_at: CREATED_AT,
  updated_at: UPDATED_AT,
};

export const AUDITOR: CustomRole = {
  id: 'b3c1d2e4-0000-4000-8000-000000000002',
  organization_id: ORGANIZATION_ID,
  name: 'Auditor',
  description: null,
  scope: RoleScope.Organization,
  permissions: [Permission.OrganizationRead, Permission.AuditLogsRead],
  members: 0,
  created_at: CREATED_AT,
  updated_at: CREATED_AT,
};

export const COMMUNITY_EMPTY: RolesResponse = {
  entitled: false,
  editable: false,
  edition: Edition.Community,
  status: LicenseStatus.Community,
  catalogue: CATALOGUE,
  roles: [],
};

export const COMMUNITY_WITH_ROLES: RolesResponse = {
  ...COMMUNITY_EMPTY,
  roles: [RELEASE_MANAGER],
};

export const ACTIVE: RolesResponse = {
  entitled: true,
  editable: true,
  edition: Edition.Enterprise,
  status: LicenseStatus.Active,
  catalogue: CATALOGUE,
  roles: [RELEASE_MANAGER, AUDITOR],
};

export const ACTIVE_EMPTY: RolesResponse = { ...ACTIVE, roles: [] };

export const ACTIVE_READ_ONLY: RolesResponse = { ...ACTIVE, editable: false };

export const GRACE: RolesResponse = { ...ACTIVE, status: LicenseStatus.Grace };

export const LAPSED: RolesResponse = {
  ...ACTIVE,
  entitled: false,
  editable: false,
  status: LicenseStatus.Lapsed,
};

export const TEAM: RolesResponse = {
  ...COMMUNITY_WITH_ROLES,
  edition: Edition.Team,
  status: LicenseStatus.Active,
};

export const TEAM_LAPSED: RolesResponse = { ...TEAM, status: LicenseStatus.Lapsed };
