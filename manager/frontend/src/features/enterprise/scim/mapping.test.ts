import { describe, expect, it } from 'bun:test';
import {
  AUDITOR,
  RELEASE_MANAGER,
  ACTIVE as ROLES_ACTIVE,
  LAPSED as ROLES_LAPSED,
} from '../roles/fixtures';
import { COMPLIANCE, CONTRACTORS, ENGINEERING, PLATFORM_ADMINS } from './fixtures';
import {
  customRoleValue,
  mappingOptions,
  mappingRequest,
  mappingValue,
  organizationRoles,
  UNMAPPED,
} from './mapping';
import { MappedRole } from './schemas';

const AUDITOR_REFERENCE = { id: AUDITOR.id, name: AUDITOR.name };

describe('mappingValue', () => {
  it('names a built-in mapping by its role, a custom one by its role id, and none as unmapped', () => {
    expect(mappingValue(ENGINEERING)).toBe(MappedRole.Member);
    expect(mappingValue(PLATFORM_ADMINS)).toBe(MappedRole.Admin);
    expect(mappingValue(COMPLIANCE)).toBe(customRoleValue(AUDITOR.id));
    expect(mappingValue(CONTRACTORS)).toBe(UNMAPPED);
  });
});

describe('mappingRequest', () => {
  it('turns each choice back into the request the server takes', () => {
    expect(mappingRequest(MappedRole.Admin)).toEqual({ role: MappedRole.Admin });
    expect(mappingRequest(MappedRole.Member)).toEqual({ role: MappedRole.Member });
    expect(mappingRequest(customRoleValue(AUDITOR.id))).toEqual({ custom_role_id: AUDITOR.id });
  });

  it('asks for no mapping when unmapped, or for anything it does not know', () => {
    expect(mappingRequest(UNMAPPED)).toBeNull();
    expect(mappingRequest('owner')).toBeNull();
  });
});

describe('organizationRoles', () => {
  it('offers only the organization-scope custom roles while licensed', () => {
    expect(organizationRoles(ROLES_ACTIVE)).toEqual([AUDITOR_REFERENCE]);
    expect(organizationRoles(ROLES_ACTIVE).map((role) => role.id)).not.toContain(
      RELEASE_MANAGER.id
    );
  });

  it('offers none without a license or before the roles load', () => {
    expect(organizationRoles(ROLES_LAPSED)).toEqual([]);
    expect(organizationRoles(null)).toEqual([]);
  });
});

describe('mappingOptions', () => {
  it('lists unmapped, the built-in roles but owner, then the custom roles', () => {
    expect(mappingOptions([AUDITOR_REFERENCE], null)).toEqual([
      { value: UNMAPPED, label: 'Unmapped' },
      { value: MappedRole.Admin, label: 'Admin' },
      { value: MappedRole.Member, label: 'Member' },
      { value: customRoleValue(AUDITOR.id), label: 'Auditor', description: 'Custom role' },
    ]);
  });

  it('keeps a held custom role choosable when it is no longer offered', () => {
    const values = mappingOptions([], AUDITOR_REFERENCE).map((option) => option.value);
    expect(values).toEqual([
      UNMAPPED,
      MappedRole.Admin,
      MappedRole.Member,
      customRoleValue(AUDITOR.id),
    ]);
  });

  it('lists a held and offered custom role once', () => {
    expect(mappingOptions([AUDITOR_REFERENCE], AUDITOR_REFERENCE)).toHaveLength(4);
  });
});
