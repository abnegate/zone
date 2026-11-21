import { RoleCode } from './schemas';

export const RoleField = {
  Name: 'name',
  Permissions: 'permissions',
} as const;

export type RoleField = (typeof RoleField)[keyof typeof RoleField];

export const NAME_REQUIRED = 'Enter a name for the role.';

export const EXCEEDS = 'Only an organization owner can widen a workspace role people already hold.';

const MESSAGES: Readonly<Record<Exclude<RoleCode, typeof RoleCode.InUse>, string>> = {
  [RoleCode.LicenseRequired]:
    'Changing roles needs a Zone Enterprise license that is active or in its grace period.',
  [RoleCode.PermissionUnknown]:
    "This server doesn't know one of the chosen permissions. Reload the page and try again.",
  [RoleCode.PermissionNotComposable]:
    "One of the chosen permissions is kept for built-in owners and admins, so it can't go in a custom role.",
  [RoleCode.PermissionOutOfScope]:
    "One of the chosen permissions doesn't apply to roles of this kind.",
  [RoleCode.NameInvalid]: NAME_REQUIRED,
  [RoleCode.NameReserved]:
    'Owner, admin, member and viewer are built-in roles. Choose another name.',
  [RoleCode.NameTaken]: 'Another role in this organization already has this name.',
  [RoleCode.Exceeds]: EXCEEDS,
};

const FIELDS: Readonly<Partial<Record<RoleCode, RoleField>>> = {
  [RoleCode.NameInvalid]: RoleField.Name,
  [RoleCode.NameReserved]: RoleField.Name,
  [RoleCode.NameTaken]: RoleField.Name,
  [RoleCode.PermissionUnknown]: RoleField.Permissions,
  [RoleCode.PermissionNotComposable]: RoleField.Permissions,
  [RoleCode.PermissionOutOfScope]: RoleField.Permissions,
};

export function inUseMessage(members: number | undefined): string {
  if (members === undefined) {
    return 'People still hold this role. Reassign them first.';
  }
  return `Reassign its ${members} ${members === 1 ? 'member' : 'members'} first.`;
}

export function refusalMessage(code: RoleCode, members?: number): string {
  return code === RoleCode.InUse ? inUseMessage(members) : MESSAGES[code];
}

export function refusalField(code: RoleCode): RoleField | null {
  return FIELDS[code] ?? null;
}
