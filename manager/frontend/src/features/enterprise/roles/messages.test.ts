import { describe, expect, it } from 'bun:test';
import { EXCEEDS, inUseMessage, RoleField, refusalField, refusalMessage } from './messages';
import { ROLE_CODES, RoleCode } from './schemas';

describe('role refusals', () => {
  it('explains every code', () => {
    for (const code of ROLE_CODES) {
      expect(refusalMessage(code, 1).length, code).toBeGreaterThan(0);
    }
  });

  it('asks for the members of a role in use to be reassigned first', () => {
    expect(refusalMessage(RoleCode.InUse, 4)).toBe('Reassign its 4 members first.');
    expect(inUseMessage(1)).toBe('Reassign its 1 member first.');
    expect(inUseMessage(undefined)).toBe('People still hold this role. Reassign them first.');
  });

  it('says only an organization owner can widen a held workspace role', () => {
    expect(refusalMessage(RoleCode.Exceeds)).toBe(EXCEEDS);
    expect(EXCEEDS).toBe(
      'Only an organization owner can widen a workspace role people already hold.'
    );
  });

  it('places name refusals on the name and permission refusals on the permissions', () => {
    for (const code of [RoleCode.NameInvalid, RoleCode.NameReserved, RoleCode.NameTaken]) {
      expect(refusalField(code), code).toBe(RoleField.Name);
    }
    for (const code of [
      RoleCode.PermissionUnknown,
      RoleCode.PermissionNotComposable,
      RoleCode.PermissionOutOfScope,
    ]) {
      expect(refusalField(code), code).toBe(RoleField.Permissions);
    }
    for (const code of [RoleCode.LicenseRequired, RoleCode.Exceeds, RoleCode.InUse]) {
      expect(refusalField(code), code).toBeNull();
    }
  });
});
