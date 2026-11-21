import type { SelectOption } from '@zone/ui';
import {
  type CustomRoleReference,
  RoleCode,
  RoleRefusal,
  type RoleScope,
  type RolesResponse,
} from '../enterprise';
import type { CustomRoleAssignment } from './organization/types';

const CUSTOM_ROLE_PREFIX = 'custom-role:';

export const CUSTOM_ROLE_DESCRIPTION = 'Custom role';

const REFUSALS: Readonly<Partial<Record<RoleCode, string>>> = {
  [RoleCode.Exceeds]:
    'That role includes permissions you don’t hold, so you can’t give it to someone else.',
  [RoleCode.LicenseRequired]:
    'Assigning a custom role needs a Zone Enterprise license that is active or in its grace period. Built-in roles can still be assigned.',
};

export interface Assignee<R extends string> {
  role: R;
  custom_role: CustomRoleReference | null;
}

export type Assignment<R extends string> = { role: R } | CustomRoleAssignment;

export function customRoleValue(id: string): string {
  return `${CUSTOM_ROLE_PREFIX}${id}`;
}

export function heldRole<R extends string>(member: Assignee<R>): string {
  return member.custom_role === null ? member.role : customRoleValue(member.custom_role.id);
}

export function customRoleOptions(
  roles: RolesResponse | null,
  scope: RoleScope,
  held: CustomRoleReference | null
): SelectOption[] {
  const offered: CustomRoleReference[] = roles?.entitled
    ? roles.roles.filter((role) => role.scope === scope)
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
  if (failure instanceof RoleRefusal) {
    return REFUSALS[failure.code] ?? failure.message;
  }
  return failure instanceof Error ? failure.message : fallback;
}
