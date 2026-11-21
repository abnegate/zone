import { type PermissionName, ROLE_BASE, RoleScope } from './schemas';
import type {
  CreateRoleRequest,
  CustomRole,
  PermissionDefinition,
  UpdateRoleRequest,
} from './types';

export interface RoleDraft {
  name: string;
  description: string;
  scope: RoleScope;
  permissions: readonly PermissionName[];
}

export function emptyDraft(): RoleDraft {
  return { name: '', description: '', scope: RoleScope.Workspace, permissions: [] };
}

export function draftOf(role: CustomRole): RoleDraft {
  const base = ROLE_BASE[role.scope];
  return {
    name: role.name,
    description: role.description ?? '',
    scope: role.scope,
    permissions: role.permissions.filter((permission) => permission !== base),
  };
}

export function choices(
  catalogue: readonly PermissionDefinition[],
  scope: RoleScope
): PermissionDefinition[] {
  return catalogue.filter((entry) => entry.scope === scope && entry.composable);
}

export function toggled(
  permissions: readonly PermissionName[],
  permission: PermissionName,
  on: boolean
): PermissionName[] {
  const rest = permissions.filter((entry) => entry !== permission);
  return on ? [...rest, permission] : rest;
}

export function rescoped(
  draft: RoleDraft,
  scope: RoleScope,
  catalogue: readonly PermissionDefinition[]
): RoleDraft {
  const allowed = new Set(choices(catalogue, scope).map((entry) => entry.name));
  return {
    ...draft,
    scope,
    permissions: draft.permissions.filter((permission) => allowed.has(permission)),
  };
}

function ordered(
  permissions: readonly PermissionName[],
  catalogue: readonly PermissionDefinition[]
): PermissionName[] {
  const chosen = new Set(permissions);
  return catalogue.map((entry) => entry.name).filter((name) => chosen.has(name));
}

function description(text: string): string | null {
  const trimmed = text.trim();
  return trimmed === '' ? null : trimmed;
}

function same(left: readonly PermissionName[], right: readonly PermissionName[]): boolean {
  const set = new Set(left);
  return left.length === right.length && right.every((entry) => set.has(entry));
}

export function nameMissing(draft: RoleDraft): boolean {
  return draft.name.trim() === '';
}

export function createRequest(
  draft: RoleDraft,
  catalogue: readonly PermissionDefinition[]
): CreateRoleRequest {
  return {
    name: draft.name.trim(),
    description: description(draft.description),
    scope: draft.scope,
    permissions: ordered(draft.permissions, catalogue),
  };
}

export function updateRequest(
  role: CustomRole,
  draft: RoleDraft,
  catalogue: readonly PermissionDefinition[]
): UpdateRoleRequest {
  const saved = draftOf(role);
  const request: UpdateRoleRequest = {};
  const name = draft.name.trim();
  if (name !== role.name) {
    request.name = name;
  }
  const text = description(draft.description);
  if (text !== role.description) {
    request.description = text;
  }
  if (!same(draft.permissions, saved.permissions)) {
    request.permissions = ordered(draft.permissions, catalogue);
  }
  return request;
}

export function changed(request: UpdateRoleRequest): boolean {
  return Object.keys(request).length > 0;
}
