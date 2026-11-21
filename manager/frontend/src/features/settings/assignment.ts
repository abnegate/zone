import type { SelectOption } from '@zone/ui';
import { ApiError } from '../../api/ApiError';
import { meetsOrganizationRole, meetsWorkspaceRole } from '../../shared/roles';
import type { WorkspaceRole } from '../auth/types';
import {
  type CustomRoleReference,
  Permission,
  RoleCode,
  RoleScope,
  type RolesResponse,
} from '../enterprise';
import {
  grantsOrganization,
  grantsWorkspace,
  ORGANIZATION_FLOOR,
  type OrganizationPermission,
  WORKSPACE_FLOOR,
  type WorkspacePermission,
} from './navigation/access';
import type { CustomRoleAssignment, OrgRole } from './organization/types';

const CUSTOM_ROLE_PREFIX = 'custom-role:';

export const CUSTOM_ROLE_DESCRIPTION = 'Custom role';

const REFUSALS: Readonly<Partial<Record<RoleCode, string>>> = {
  [RoleCode.Exceeds]:
    "That role includes permissions you don't hold, so you can't give it to someone else.",
  [RoleCode.LicenseRequired]:
    'Assigning a custom role needs a Zone Enterprise license that is active or in its grace period. Built-in roles can still be assigned.',
};

export interface RoleOption<R extends string> {
  value: R;
  label: string;
}

export const ORGANIZATION_ROLES: readonly RoleOption<OrgRole>[] = [
  { value: 'member', label: 'Member' },
  { value: 'admin', label: 'Admin' },
  { value: 'owner', label: 'Owner' },
];

export const WORKSPACE_ROLES: readonly RoleOption<WorkspaceRole>[] = [
  { value: 'viewer', label: 'Viewer' },
  { value: 'member', label: 'Member' },
  { value: 'admin', label: 'Admin' },
  { value: 'owner', label: 'Owner' },
];

export interface Assignee<R extends string> {
  role: R;
  custom_role: CustomRoleReference | null;
  permissions?: readonly string[] | undefined;
}

export interface Standing<R extends string> {
  role: R;
  permissions: readonly string[] | undefined;
}

export interface Ladder<R extends string, P extends string> {
  scope: RoleScope;
  roles: readonly RoleOption<R>[];
  privileged: readonly R[];
  floor: Readonly<Record<P, R>>;
  meets: (role: R, required: R) => boolean;
  grants: (standing: Standing<R>, name: P) => boolean;
  create: P;
  update: P;
  remove: P;
  administrators: P;
}

export const ORGANIZATION_LADDER: Ladder<OrgRole, OrganizationPermission> = {
  scope: RoleScope.Organization,
  roles: ORGANIZATION_ROLES,
  privileged: ['admin', 'owner'],
  floor: ORGANIZATION_FLOOR,
  meets: meetsOrganizationRole,
  grants: (standing, name) => grantsOrganization(standing.permissions, standing.role, name),
  create: Permission.MembersCreate,
  update: Permission.MembersUpdate,
  remove: Permission.MembersDelete,
  administrators: Permission.AdministratorsUpdate,
};

export const WORKSPACE_LADDER: Ladder<WorkspaceRole, WorkspacePermission> = {
  scope: RoleScope.Workspace,
  roles: WORKSPACE_ROLES,
  privileged: ['admin', 'owner'],
  floor: WORKSPACE_FLOOR,
  meets: meetsWorkspaceRole,
  grants: (standing, name) => grantsWorkspace(standing.permissions, standing.role, name),
  create: Permission.WorkspaceMembersCreate,
  update: Permission.WorkspaceMembersUpdate,
  remove: Permission.WorkspaceMembersDelete,
  administrators: Permission.WorkspaceAdministratorsUpdate,
};

export type Assignment<R extends string> = { role: R } | CustomRoleAssignment;

export function covers(held: readonly string[] | undefined, granted: readonly string[]): boolean {
  return held === undefined || granted.every((name) => held.includes(name));
}

export function builtInGrant<R extends string, P extends string>(
  ladder: Ladder<R, P>,
  role: R
): P[] {
  return (Object.keys(ladder.floor) as P[]).filter((name) =>
    ladder.meets(role, ladder.floor[name])
  );
}

export function grantableRoles<R extends string, P extends string>(
  ladder: Ladder<R, P>,
  standing: Standing<R>
): RoleOption<R>[] {
  const administers = ladder.grants(standing, ladder.administrators);
  return ladder.roles.filter(
    ({ value }) =>
      (administers || !ladder.privileged.includes(value)) &&
      covers(standing.permissions, builtInGrant(ladder, value))
  );
}

export function reaches<R extends string, P extends string>(
  ladder: Ladder<R, P>,
  standing: Standing<R>,
  member: Assignee<R>
): boolean {
  const privileged = member.custom_role === null && ladder.privileged.includes(member.role);
  return (
    (!privileged || ladder.grants(standing, ladder.administrators)) &&
    covers(standing.permissions, member.permissions ?? builtInGrant(ladder, member.role))
  );
}

export function roleLabel<R extends string>(member: Assignee<R>): string {
  return member.custom_role?.name ?? member.role.charAt(0).toUpperCase() + member.role.slice(1);
}

export function roleBadgeClass<R extends string>(member: Assignee<R>): string {
  return member.custom_role === null
    ? `role-badge role-badge-${member.role}`
    : 'role-badge role-badge-custom';
}

export function customRoleValue(id: string): string {
  return `${CUSTOM_ROLE_PREFIX}${id}`;
}

export function heldRole<R extends string>(member: Assignee<R>): string {
  return member.custom_role === null ? member.role : customRoleValue(member.custom_role.id);
}

export function customRoleOptions(
  roles: RolesResponse | null,
  scope: RoleScope,
  held: CustomRoleReference | null,
  permissions?: readonly string[]
): SelectOption[] {
  const offered: CustomRoleReference[] = roles?.entitled
    ? roles.roles.filter((role) => role.scope === scope && covers(permissions, role.permissions))
    : [];
  const choices =
    held === null || offered.some((role) => role.id === held.id) ? offered : [...offered, held];
  return choices.map((role) => ({
    value: customRoleValue(role.id),
    label: role.name,
    description: CUSTOM_ROLE_DESCRIPTION,
  }));
}

export function assignmentOf<R extends string>(value: string): Assignment<R> {
  return value.startsWith(CUSTOM_ROLE_PREFIX)
    ? { custom_role_id: value.slice(CUSTOM_ROLE_PREFIX.length) }
    : { role: value as R };
}

export function assignsCustomRole<R extends string>(
  assignment: Assignment<R>
): assignment is CustomRoleAssignment {
  return 'custom_role_id' in assignment;
}

export function assignmentFailure(failure: unknown, fallback: string): string {
  if (failure instanceof ApiError && failure.code !== undefined) {
    return REFUSALS[failure.code as RoleCode] ?? failure.message;
  }
  return failure instanceof Error ? failure.message : fallback;
}
