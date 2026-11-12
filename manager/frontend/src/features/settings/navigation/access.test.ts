import { describe, expect, it } from 'bun:test';
import type { OrgRole, WorkspaceRole } from '../../../types';
import { SOURCES_WRITE_ROLE } from '../../sources/access';
import {
  allowsOrganization,
  allowsWorkspace,
  ORGANIZATION_ADMIN_ROLE,
  WORKSPACE_EDIT_ROLE,
} from './access';

const ORGANIZATION_ROLES = ['member', 'admin', 'owner'] as const satisfies readonly OrgRole[];
const WORKSPACE_ROLES = [
  'viewer',
  'member',
  'admin',
  'owner',
] as const satisfies readonly WorkspaceRole[];

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

describe('edit roles', () => {
  it('names the workspace role each editable section needs, leaving members to manage its own', () => {
    expect(WORKSPACE_EDIT_ROLE).toEqual({
      theme: 'member',
      ai: 'admin',
      folders: 'member',
      mcp: 'admin',
      sources: 'member',
    });
    expect(Object.keys(WORKSPACE_EDIT_ROLE).sort()).toEqual([
      'ai',
      'folders',
      'mcp',
      'sources',
      'theme',
    ]);
  });

  it('edits sources with the same role the sources feature writes them with', () => {
    expect(WORKSPACE_EDIT_ROLE.sources).toBe(SOURCES_WRITE_ROLE);
  });

  it('requires an organization admin for the admin-only organization sections', () => {
    expect(ORGANIZATION_ADMIN_ROLE).toBe('admin');
  });
});
