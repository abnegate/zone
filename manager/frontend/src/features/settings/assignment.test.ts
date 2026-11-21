import { describe, expect, it } from 'bun:test';
import { ApiError } from '../../api/ApiError';
import { ACTIVE, AUDITOR, LAPSED, RELEASE_MANAGER } from '../enterprise/roles/fixtures';
import { RoleRefusal } from '../enterprise/roles/RoleRefusal';
import { RoleCode, RoleScope } from '../enterprise/roles/schemas';
import {
  assignmentFailure,
  assignmentOf,
  assignsCustomRole,
  CUSTOM_ROLE_DESCRIPTION,
  customRoleOptions,
  customRoleValue,
  heldRole,
} from './assignment';

const auditor = { id: AUDITOR.id, name: AUDITOR.name };

describe('customRoleOptions', () => {
  it('offers the licensed roles of the scope, each marked as a custom role', () => {
    expect(customRoleOptions(ACTIVE, RoleScope.Organization, null)).toEqual([
      {
        value: customRoleValue(AUDITOR.id),
        label: AUDITOR.name,
        description: CUSTOM_ROLE_DESCRIPTION,
      },
    ]);
    expect(customRoleOptions(ACTIVE, RoleScope.Workspace, null).map(({ label }) => label)).toEqual([
      RELEASE_MANAGER.name,
    ]);
  });

  it('offers nothing unlicensed or unloaded but the role already held', () => {
    expect(customRoleOptions(LAPSED, RoleScope.Organization, null)).toEqual([]);
    expect(customRoleOptions(null, RoleScope.Organization, null)).toEqual([]);
    expect(customRoleOptions(LAPSED, RoleScope.Organization, auditor)).toEqual([
      {
        value: customRoleValue(AUDITOR.id),
        label: AUDITOR.name,
        description: CUSTOM_ROLE_DESCRIPTION,
      },
    ]);
  });

  it('never offers the held role twice', () => {
    expect(customRoleOptions(ACTIVE, RoleScope.Organization, auditor)).toHaveLength(1);
  });
});

describe('assignments', () => {
  it('turns a choice into the request the member routes take', () => {
    expect(assignmentOf('admin')).toEqual({ role: 'admin' });
    expect(assignmentOf(customRoleValue(AUDITOR.id))).toEqual({ custom_role_id: AUDITOR.id });
    expect(assignsCustomRole(assignmentOf(customRoleValue(AUDITOR.id)))).toBe(true);
    expect(assignsCustomRole(assignmentOf('member'))).toBe(false);
  });

  it('chooses the custom role a member holds over its rank', () => {
    expect(heldRole({ role: 'member', custom_role: null })).toBe('member');
    expect(heldRole({ role: 'member', custom_role: auditor })).toBe(customRoleValue(AUDITOR.id));
  });
});

describe('assignmentFailure', () => {
  it('explains the refusals a custom assignment can meet', () => {
    expect(
      assignmentFailure(new RoleRefusal({ error: 'x', code: RoleCode.Exceeds }, 403), 'fallback')
    ).toMatch(/permissions you don’t hold/);
    expect(
      assignmentFailure(
        new RoleRefusal({ error: 'x', code: RoleCode.LicenseRequired }, 403),
        'fallback'
      )
    ).toMatch(/Built-in roles can still be assigned/);
  });

  it('keeps the server reason for any other failure', () => {
    expect(
      assignmentFailure(
        new RoleRefusal({ error: 'Name taken', code: RoleCode.NameTaken }, 409),
        'x'
      )
    ).toBe('Name taken');
    expect(assignmentFailure(new ApiError('Role not found', 404), 'fallback')).toBe(
      'Role not found'
    );
    expect(assignmentFailure('nonsense', 'fallback')).toBe('fallback');
  });
});
