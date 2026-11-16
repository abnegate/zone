import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { renderHook } from '@testing-library/react';
import { PERMISSIONS } from '../../../shared/types/permissions';
import type { WorkspaceRoleState } from './useWorkspaceRole';

interface AccessContext {
  currentOrganization: { id: string; role: string } | null;
  currentWorkspace: { id: string } | null;
  loading: boolean;
  resolvingRole: boolean;
}

const workspaceContext: AccessContext = {
  currentOrganization: { id: 'organization-1', role: 'member' },
  currentWorkspace: { id: 'workspace-1' },
  loading: false,
  resolvingRole: false,
};

const workspaceRole: WorkspaceRoleState = { role: 'viewer', resolving: false };

let user: { is_admin: boolean } | null = null;

let permissions: string[] = [];

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => workspaceContext,
}));

mock.module('./useWorkspaceRole', () => ({
  useWorkspaceRole: () => workspaceRole,
}));

mock.module('../../auth/context', () => ({
  useAuth: () => ({
    user,
    hasPermission: (permission: string) => permissions.includes(permission),
  }),
}));

let useSettingsAccess: typeof import('./useSettingsAccess').useSettingsAccess;

beforeAll(async () => {
  ({ useSettingsAccess } = await import('./useSettingsAccess'));
});

afterAll(() => {
  mock.restore();
});

describe('useSettingsAccess', () => {
  beforeEach(() => {
    user = null;
    permissions = [];
    workspaceContext.loading = false;
    workspaceContext.resolvingRole = false;
  });

  it('marks a server admin as administering the server', () => {
    user = { is_admin: true };

    const { result } = renderHook(() => useSettingsAccess());

    expect(result.current.access.administersServer).toBe(true);
  });

  it('does not let an ordinary user administer the server', () => {
    user = { is_admin: false };

    const { result } = renderHook(() => useSettingsAccess());

    expect(result.current.access.administersServer).toBe(false);
  });

  it('does not let a signed-out visitor administer the server', () => {
    const { result } = renderHook(() => useSettingsAccess());

    expect(result.current.access.administersServer).toBe(false);
  });

  it('keeps server administration apart from organization roles and permissions', () => {
    user = { is_admin: false };
    permissions = [PERMISSIONS.MODELS.READ, PERMISSIONS.SOURCES.READ];

    const { result } = renderHook(() => useSettingsAccess());

    expect(result.current.access).toEqual({
      organization: true,
      organizationRole: 'member',
      workspace: true,
      workspaceRole: 'viewer',
      installsModels: true,
      readsSources: true,
      administersServer: false,
    });
  });
});
