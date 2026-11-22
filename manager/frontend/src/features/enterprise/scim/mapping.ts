import type { SelectOption } from '@zone/ui';
import { RoleScope } from '../roles/schemas';
import type { CustomRoleReference, RolesResponse } from '../roles/types';
import { CUSTOM_ROLE_DESCRIPTION, MAPPED_ROLE_LABELS, UNMAPPED_LABEL } from './labels';
import { MAPPED_ROLES, type MappedRole } from './schemas';
import type { GroupResponse, MappingRequest } from './types';

export const UNMAPPED = 'unmapped';

const CUSTOM_ROLE_PREFIX = 'custom-role:';

export function customRoleValue(id: string): string {
  return `${CUSTOM_ROLE_PREFIX}${id}`;
}

export function mappingValue(group: GroupResponse): string {
  if (group.custom_role !== null) {
    return customRoleValue(group.custom_role.id);
  }
  return group.role ?? UNMAPPED;
}

export function mappingRequest(value: string): MappingRequest | null {
  if (value.startsWith(CUSTOM_ROLE_PREFIX)) {
    return { custom_role_id: value.slice(CUSTOM_ROLE_PREFIX.length) };
  }
  const role = MAPPED_ROLES.find((candidate) => candidate === value);
  return role === undefined ? null : { role };
}

export function organizationRoles(roles: RolesResponse | null): CustomRoleReference[] {
  if (!roles?.entitled) {
    return [];
  }
  return roles.roles
    .filter((role) => role.scope === RoleScope.Organization)
    .map(({ id, name }) => ({ id, name }));
}

function builtIn(role: MappedRole): SelectOption {
  return { value: role, label: MAPPED_ROLE_LABELS[role] };
}

export function mappingOptions(
  offered: readonly CustomRoleReference[],
  held: CustomRoleReference | null
): SelectOption[] {
  const custom =
    held === null || offered.some((role) => role.id === held.id) ? offered : [...offered, held];
  return [
    { value: UNMAPPED, label: UNMAPPED_LABEL },
    ...MAPPED_ROLES.map(builtIn),
    ...custom.map((role) => ({
      value: customRoleValue(role.id),
      label: role.name,
      description: CUSTOM_ROLE_DESCRIPTION,
    })),
  ];
}
