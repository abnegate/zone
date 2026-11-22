import { describe, expect, it } from 'bun:test';
import { ApiError } from '../../api/ApiError';
import { ACTIVE, AUDITOR, LAPSED, RELEASE_MANAGER } from '../enterprise/roles/fixtures';
import { RoleRefusal } from '../enterprise/roles/RoleRefusal';
import { Permission, RoleCode, RoleScope } from '../enterprise/roles/schemas';
import {
  assignmentFailure,
  assignmentOf,
  assignsCustomRole,
  builtInGrant,
  CUSTOM_ROLE_DESCRIPTION,
  covers,
  customRoleOptions,
  customRoleValue,
  grantableRoles,
  heldRole,
  ORGANIZATION_LADDER,
  reaches,
  WORKSPACE_LADDER,
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

describe('member controls', () => {
  const values = (options: { value: string }[]) => options.map(({ value }) => value);

  it('covers everything when the holder is unknown, and otherwise only what they hold', () => {
    expect(covers(undefined, [Permission.AuditLogsRead])).toBe(true);
    expect(covers([Permission.OrganizationRead], [Permission.AuditLogsRead])).toBe(false);
    expect(covers(AUDITOR.permissions, AUDITOR.permissions)).toBe(true);
  });

  it('builds the built-in grants from the floors the server uses', () => {
    expect(builtInGrant(WORKSPACE_LADDER, 'member')).toEqual([
      Permission.WorkspaceRead,
      Permission.WorkspaceWrite,
    ]);
    expect(builtInGrant(ORGANIZATION_LADDER, 'owner')).toContain(Permission.AdministratorsUpdate);
    expect(builtInGrant(ORGANIZATION_LADDER, 'admin')).not.toContain(
      Permission.AdministratorsUpdate
    );
  });

  it('offers by rank when the permissions are unknown, as before custom roles', () => {
    const unknown = (role: 'admin' | 'owner') => ({ role, permissions: undefined });
    expect(values(grantableRoles(ORGANIZATION_LADDER, unknown('admin')))).toEqual(['member']);
    expect(values(grantableRoles(ORGANIZATION_LADDER, unknown('owner')))).toEqual([
      'member',
      'admin',
      'owner',
    ]);
    expect(values(grantableRoles(WORKSPACE_LADDER, unknown('admin')))).toEqual([
      'viewer',
      'member',
    ]);
  });

  it('offers a custom holder only the built-in roles their grant covers', () => {
    const holder = {
      role: 'viewer' as const,
      permissions: [Permission.WorkspaceRead, Permission.WorkspaceMembersUpdate],
    };
    expect(values(grantableRoles(WORKSPACE_LADDER, holder))).toEqual(['viewer']);
  });

  it('reaches an admin or owner only through the administrators permission', () => {
    const admin = { role: 'admin' as const, permissions: undefined };
    const owner = { role: 'owner' as const, permissions: undefined };
    const target = { role: 'admin' as const, custom_role: null };
    expect(reaches(WORKSPACE_LADDER, admin, target)).toBe(false);
    expect(reaches(WORKSPACE_LADDER, owner, target)).toBe(true);
    expect(
      reaches(WORKSPACE_LADDER, admin, { ...target, custom_role: { id: 'r', name: 'Everything' } })
    ).toBe(true);
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
    ).toMatch(/permissions you don't hold/);
    expect(
      assignmentFailure(
        new RoleRefusal({ error: 'x', code: RoleCode.LicenseRequired }, 403),
        'fallback'
      )
    ).toMatch(/Built-in roles can still be assigned/);
  });

  it('explains a refusal the member routes send as a plain coded error', () => {
    expect(assignmentFailure(new ApiError('x', 403, RoleCode.Exceeds), 'fallback')).toMatch(
      /permissions you don't hold/
    );
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
