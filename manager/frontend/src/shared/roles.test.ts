import { describe, expect, it } from 'bun:test';
import type { OrgRole, WorkspaceRole } from '../types';
import {
  meetsOrganizationRole,
  meetsWorkspaceRole,
  ORGANIZATION_RANK,
  WORKSPACE_RANK,
} from './roles';

const ORGANIZATION_ROLES = ['member', 'admin', 'owner'] as const satisfies readonly OrgRole[];
const WORKSPACE_ROLES = [
  'viewer',
  'member',
  'admin',
  'owner',
] as const satisfies readonly WorkspaceRole[];

describe('role ranks', () => {
  it('ranks organization roles member, admin, owner', () => {
    expect(ORGANIZATION_RANK).toEqual({ member: 0, admin: 1, owner: 2 });
  });

  it('ranks workspace roles viewer, member, admin, owner', () => {
    expect(WORKSPACE_RANK).toEqual({ viewer: 0, member: 1, admin: 2, owner: 3 });
  });
});

describe('meetsOrganizationRole', () => {
  it('accepts a role at or above the required one and refuses one below', () => {
    for (const [roleIndex, role] of ORGANIZATION_ROLES.entries()) {
      for (const [requiredIndex, required] of ORGANIZATION_ROLES.entries()) {
        expect(meetsOrganizationRole(role, required), `${role} against ${required}`).toBe(
          roleIndex >= requiredIndex
        );
      }
    }
  });
});

describe('meetsWorkspaceRole', () => {
  it('accepts a role at or above the required one and refuses one below', () => {
    for (const [roleIndex, role] of WORKSPACE_ROLES.entries()) {
      for (const [requiredIndex, required] of WORKSPACE_ROLES.entries()) {
        expect(meetsWorkspaceRole(role, required), `${role} against ${required}`).toBe(
          roleIndex >= requiredIndex
        );
      }
    }
  });
});
