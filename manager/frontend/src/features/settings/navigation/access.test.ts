import { describe, expect, it } from 'bun:test';
import type { OrgRole, WorkspaceRole } from '../../../types';
import {
  ORGANIZATION_PERMISSIONS,
  Permission,
  type PermissionName,
  WORKSPACE_PERMISSIONS,
} from '../../enterprise/roles/schemas';
import { SOURCES_WRITE_ROLE } from '../../sources/access';
import {
  allowsOrganization,
  allowsWorkspace,
  grantsOrganization,
  grantsWorkspace,
  ORGANIZATION_FLOOR,
  WORKSPACE_EDIT_PERMISSION,
  WORKSPACE_FLOOR,
} from './access';

const ORGANIZATION_ROLES = ['member', 'admin', 'owner'] as const satisfies readonly OrgRole[];
const WORKSPACE_ROLES = [
  'viewer',
  'member',
  'admin',
  'owner',
] as const satisfies readonly WorkspaceRole[];

const ORGANIZATION_OWNER_ONLY: readonly PermissionName[] = [
  Permission.OrganizationDelete,
  Permission.AdministratorsUpdate,
];
const WORKSPACE_OWNER_ONLY: readonly PermissionName[] = [
  Permission.WorkspaceDelete,
  Permission.WorkspaceAdministratorsUpdate,
];

describe('allowsOrganization', () => {
  it('allows an unknown role whatever is required, denying only an explicit one', () => {
    for (const required of ORGANIZATION_ROLES) {
      expect(allowsOrganization(undefined, required), `undefined against ${required}`).toBe(true);
    }
  });

  it('ranks an explicit role against the required one', () => {
    expect(allowsOrganization('member', 'admin')).toBe(false);
    expect(allowsOrganization('admin', 'admin')).toBe(true);
    expect(allowsOrganization('owner', 'admin')).toBe(true);
    expect(allowsOrganization('admin', 'owner')).toBe(false);
    expect(allowsOrganization('member', 'member')).toBe(true);
  });
});

describe('allowsWorkspace', () => {
  it('allows an unknown role whatever is required, denying only an explicit one', () => {
    for (const required of WORKSPACE_ROLES) {
      expect(allowsWorkspace(undefined, required), `undefined against ${required}`).toBe(true);
    }
  });

  it('ranks an explicit role against the required one', () => {
    expect(allowsWorkspace('viewer', 'member')).toBe(false);
    expect(allowsWorkspace('member', 'member')).toBe(true);
    expect(allowsWorkspace('member', 'admin')).toBe(false);
    expect(allowsWorkspace('admin', 'admin')).toBe(true);
    expect(allowsWorkspace('owner', 'admin')).toBe(true);
    expect(allowsWorkspace('admin', 'owner')).toBe(false);
  });
});

describe('built-in floors', () => {
  it('names the lowest built-in organization role holding each organization permission', () => {
    expect(Object.keys(ORGANIZATION_FLOOR).sort()).toEqual([...ORGANIZATION_PERMISSIONS].sort());
    for (const name of ORGANIZATION_PERMISSIONS) {
      const expected =
        name === Permission.OrganizationRead
          ? 'member'
          : ORGANIZATION_OWNER_ONLY.includes(name)
            ? 'owner'
            : 'admin';
      expect(ORGANIZATION_FLOOR[name], name).toBe(expected);
    }
  });

  it('names the lowest built-in workspace role holding each workspace permission', () => {
    expect(Object.keys(WORKSPACE_FLOOR).sort()).toEqual([...WORKSPACE_PERMISSIONS].sort());
    for (const name of WORKSPACE_PERMISSIONS) {
      const expected =
        name === Permission.WorkspaceRead
          ? 'viewer'
          : name === Permission.WorkspaceWrite
            ? 'member'
            : WORKSPACE_OWNER_ONLY.includes(name)
              ? 'owner'
              : 'admin';
      expect(WORKSPACE_FLOOR[name], name).toBe(expected);
    }
  });
});

describe('grantsOrganization', () => {
  it('tests the permission name when the server sends the permissions', () => {
    const auditor = [Permission.OrganizationRead, Permission.AuditLogsRead];
    expect(grantsOrganization(auditor, 'member', Permission.AuditLogsRead)).toBe(true);
    expect(grantsOrganization(auditor, 'member', Permission.DevicesRead)).toBe(false);
    expect(grantsOrganization([], 'owner', Permission.AuditLogsRead)).toBe(false);
  });

  it('falls back to the rank check when the server sends no permissions', () => {
    for (const role of [...ORGANIZATION_ROLES, undefined]) {
      for (const name of ORGANIZATION_PERMISSIONS) {
        expect(grantsOrganization(undefined, role, name), `${role} for ${name}`).toBe(
          allowsOrganization(role, ORGANIZATION_FLOOR[name])
        );
      }
    }
  });

  it('keeps every admin-only gate of today for a role without permissions', () => {
    for (const role of [...ORGANIZATION_ROLES, undefined]) {
      expect(grantsOrganization(undefined, role, Permission.RolesUpdate), `${role}`).toBe(
        allowsOrganization(role, 'admin')
      );
    }
  });
});

describe('grantsWorkspace', () => {
  it('tests the permission name when the server sends the permissions', () => {
    const editor = [Permission.WorkspaceRead, Permission.WorkspaceAiSettingsUpdate];
    expect(grantsWorkspace(editor, 'viewer', Permission.WorkspaceAiSettingsUpdate)).toBe(true);
    expect(grantsWorkspace(editor, 'viewer', Permission.WorkspaceWrite)).toBe(false);
    expect(grantsWorkspace([], 'owner', Permission.WorkspaceWrite)).toBe(false);
  });

  it('falls back to the rank check when the server sends no permissions', () => {
    for (const role of [...WORKSPACE_ROLES, undefined]) {
      for (const name of WORKSPACE_PERMISSIONS) {
        expect(grantsWorkspace(undefined, role, name), `${role} for ${name}`).toBe(
          allowsWorkspace(role, WORKSPACE_FLOOR[name])
        );
      }
    }
  });
});

describe('edit permissions', () => {
  it('names the workspace permission each editable section needs, leaving members to manage its own', () => {
    expect(WORKSPACE_EDIT_PERMISSION).toEqual({
      theme: Permission.WorkspaceWrite,
      ai: Permission.WorkspaceAiSettingsUpdate,
      folders: Permission.WorkspaceWrite,
      mcp: Permission.WorkspaceMcpServersUpdate,
      sources: Permission.WorkspaceWrite,
    });
  });

  it('keeps the built-in role each section needed before permissions', () => {
    const floors = Object.fromEntries(
      Object.entries(WORKSPACE_EDIT_PERMISSION).map(([section, name]) => [
        section,
        WORKSPACE_FLOOR[name],
      ])
    );
    expect(floors).toEqual({
      theme: 'member',
      ai: 'admin',
      folders: 'member',
      mcp: 'admin',
      sources: 'member',
    });
  });

  it('edits sources with the same role the sources feature writes them with', () => {
    expect(WORKSPACE_FLOOR[WORKSPACE_EDIT_PERMISSION.sources]).toBe(SOURCES_WRITE_ROLE);
  });
});
