import { describe, expect, it } from 'bun:test';
import { ACTIVE, AUDITOR } from './fixtures';
import {
  CreateRoleRequestSchema,
  ORGANIZATION_PERMISSIONS,
  PERMISSION_REQUIREMENTS,
  PERMISSIONS,
  Permission,
  ROLE_BASE,
  ROLE_CODES,
  RoleCode,
  RoleRefusalSchema,
  RoleScope,
  RolesResponseSchema,
  UpdateRoleRequestSchema,
  WORKSPACE_PERMISSIONS,
} from './schemas';

describe('the permission catalogue', () => {
  it('lists every organization and workspace permission once', () => {
    expect([...PERMISSIONS].sort()).toEqual(Object.values(Permission).sort());
    expect(new Set(PERMISSIONS).size).toBe(PERMISSIONS.length);
  });

  it('names permissions resource:action', () => {
    for (const name of PERMISSIONS) {
      expect(name).toMatch(/^[a-z_]+:[a-z]+$/);
    }
  });

  it('keeps the workspace permissions apart from the organization ones', () => {
    const organization = new Set<string>(ORGANIZATION_PERMISSIONS);
    expect(WORKSPACE_PERMISSIONS.filter((name) => organization.has(name))).toEqual([]);
  });

  it('gives every scope its read permission as the base', () => {
    expect(ROLE_BASE).toEqual({
      [RoleScope.Organization]: Permission.OrganizationRead,
      [RoleScope.Workspace]: Permission.WorkspaceRead,
    });
    expect(ORGANIZATION_PERMISSIONS).toContain(ROLE_BASE[RoleScope.Organization]);
    expect(WORKSPACE_PERMISSIONS).toContain(ROLE_BASE[RoleScope.Workspace]);
  });

  it('requires only a composable permission of the same scope', () => {
    const bases = new Set<string>(Object.values(ROLE_BASE));
    const organization = new Set<string>(ORGANIZATION_PERMISSIONS);
    for (const [permission, needs] of Object.entries(PERMISSION_REQUIREMENTS)) {
      expect(bases.has(needs), permission).toBe(false);
      expect(organization.has(needs), permission).toBe(organization.has(permission));
    }
    expect(PERMISSION_REQUIREMENTS).toEqual({ [Permission.SpendUpdate]: Permission.SpendRead });
  });

  it('models every role code once', () => {
    expect([...ROLE_CODES].sort()).toEqual(Object.values(RoleCode).sort());
  });
});

describe('the role contract', () => {
  it('reads a roles response', () => {
    expect(RolesResponseSchema.parse(ACTIVE)).toEqual(ACTIVE);
  });

  it('refuses a permission the catalogue does not hold', () => {
    expect(
      RolesResponseSchema.safeParse({
        ...ACTIVE,
        roles: [{ ...AUDITOR, permissions: ['models:create'] }],
      }).success
    ).toBe(false);
  });

  it('creates with an optional description', () => {
    const base = { name: 'Auditor', scope: RoleScope.Organization, permissions: [] };
    expect(CreateRoleRequestSchema.safeParse(base).success).toBe(true);
    expect(CreateRoleRequestSchema.safeParse({ ...base, description: null }).success).toBe(true);
  });

  it('updates sparsely and never the scope', () => {
    expect(UpdateRoleRequestSchema.safeParse({}).success).toBe(true);
    expect(UpdateRoleRequestSchema.safeParse({ scope: RoleScope.Workspace }).success).toBe(false);
  });

  it('reads the member count only where the refusal carries one', () => {
    expect(RoleRefusalSchema.parse({ error: 'In use', code: RoleCode.InUse, members: 2 })).toEqual({
      error: 'In use',
      code: RoleCode.InUse,
      members: 2,
    });
    expect(RoleRefusalSchema.safeParse({ error: 'Not found' }).success).toBe(false);
  });
});
