import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import { clientModule } from '../../../test/client';
import type {
  Workspace,
  WorkspaceMember,
  WorkspaceMembersResponse,
  WorkspaceRole,
} from '../../../types';
import type { useWorkspaceRole as useWorkspaceRoleType } from './useWorkspaceRole';

const mockGetWorkspaceMembers = mock<(workspaceId: string) => Promise<WorkspaceMembersResponse>>();

mock.module('../../../api/client', () =>
  clientModule({ getWorkspaceMembers: mockGetWorkspaceMembers })
);

const workspaceContext: { currentWorkspace: Workspace | null } = { currentWorkspace: null };

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => workspaceContext,
}));

const authContext: { user: { id: string } | null } = { user: null };

mock.module('../../auth/context', () => ({
  useAuth: () => authContext,
}));

let useWorkspaceRole: typeof useWorkspaceRoleType;

beforeAll(async () => {
  ({ useWorkspaceRole } = await import('./useWorkspaceRole'));
});

afterAll(() => {
  mock.restore();
});

const USER_ID = 'user-1';

const workspace = (id: string): Workspace => ({
  id,
  organization_id: 'organization-1',
  name: id,
  slug: id,
  description: null,
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
});

const member = (userId: string, workspaceId: string, role: WorkspaceRole): WorkspaceMember => ({
  id: `${workspaceId}-${userId}`,
  user_id: userId,
  workspace_id: workspaceId,
  role,
  email: `${userId}@example.com`,
  display_name: null,
  joined_at: '2024-01-01T00:00:00Z',
});

interface Deferred {
  promise: Promise<WorkspaceMembersResponse>;
  resolve: (response: WorkspaceMembersResponse) => void;
}

function deferred(): Deferred {
  let resolve: (response: WorkspaceMembersResponse) => void = () => {};
  const promise = new Promise<WorkspaceMembersResponse>((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
}

describe('useWorkspaceRole', () => {
  beforeEach(() => {
    mockGetWorkspaceMembers.mockReset();
    workspaceContext.currentWorkspace = workspace('workspace-a');
    authContext.user = { id: USER_ID };
  });

  it('resolves the caller role from the workspace members list', async () => {
    mockGetWorkspaceMembers.mockResolvedValue({
      members: [
        member('someone-else', 'workspace-a', 'owner'),
        member(USER_ID, 'workspace-a', 'admin'),
      ],
    });

    const { result } = renderHook(() => useWorkspaceRole());

    expect(result.current).toEqual({ role: undefined, resolving: true });
    await waitFor(() => {
      expect(result.current).toEqual({ role: 'admin', resolving: false });
    });
    expect(mockGetWorkspaceMembers.mock.calls).toEqual([['workspace-a']]);
  });

  it('settles undefined when the caller is not among the members', async () => {
    mockGetWorkspaceMembers.mockResolvedValue({
      members: [member('someone-else', 'workspace-a', 'owner')],
    });

    const { result } = renderHook(() => useWorkspaceRole());

    await waitFor(() => {
      expect(result.current.resolving).toBe(false);
    });
    expect(result.current.role).toBeUndefined();
  });

  it('refetches on a workspace switch and ignores a late answer for the previous workspace', async () => {
    const first = deferred();
    const second = deferred();
    mockGetWorkspaceMembers.mockImplementation((workspaceId) =>
      workspaceId === 'workspace-a' ? first.promise : second.promise
    );

    const { result, rerender } = renderHook(() => useWorkspaceRole());
    expect(result.current).toEqual({ role: undefined, resolving: true });

    workspaceContext.currentWorkspace = workspace('workspace-b');
    rerender();

    expect(mockGetWorkspaceMembers.mock.calls).toEqual([['workspace-a'], ['workspace-b']]);
    expect(result.current).toEqual({ role: undefined, resolving: true });

    await act(async () => {
      second.resolve({ members: [member(USER_ID, 'workspace-b', 'viewer')] });
      await second.promise;
    });

    expect(result.current).toEqual({ role: 'viewer', resolving: false });

    await act(async () => {
      first.resolve({ members: [member(USER_ID, 'workspace-a', 'owner')] });
      await first.promise;
    });

    expect(result.current, 'the previous workspace answer must not replace the new one').toEqual({
      role: 'viewer',
      resolving: false,
    });
  });

  it('reports resolving again after a switch until the new workspace answers', async () => {
    mockGetWorkspaceMembers.mockResolvedValueOnce({
      members: [member(USER_ID, 'workspace-a', 'owner')],
    });
    const second = deferred();
    mockGetWorkspaceMembers.mockReturnValueOnce(second.promise);

    const { result, rerender } = renderHook(() => useWorkspaceRole());
    await waitFor(() => {
      expect(result.current).toEqual({ role: 'owner', resolving: false });
    });

    workspaceContext.currentWorkspace = workspace('workspace-b');
    rerender();

    expect(result.current, 'the old role must not leak into the new workspace').toEqual({
      role: undefined,
      resolving: true,
    });

    await act(async () => {
      second.resolve({ members: [member(USER_ID, 'workspace-b', 'member')] });
      await second.promise;
    });

    expect(result.current).toEqual({ role: 'member', resolving: false });
  });

  it('settles undefined and stops resolving when the members read fails', async () => {
    mockGetWorkspaceMembers.mockRejectedValue(new Error('Failed to fetch members: 500'));

    const { result } = renderHook(() => useWorkspaceRole());

    await waitFor(() => {
      expect(result.current.resolving).toBe(false);
    });
    expect(result.current.role).toBeUndefined();
  });

  it('neither fetches nor resolves without a workspace', async () => {
    workspaceContext.currentWorkspace = null;

    const { result } = renderHook(() => useWorkspaceRole());

    expect(result.current).toEqual({ role: undefined, resolving: false });
    await Promise.resolve();
    expect(mockGetWorkspaceMembers).not.toHaveBeenCalled();
  });

  it('neither fetches nor resolves without a signed-in user', async () => {
    authContext.user = null;

    const { result } = renderHook(() => useWorkspaceRole());

    expect(result.current).toEqual({ role: undefined, resolving: false });
    await Promise.resolve();
    expect(mockGetWorkspaceMembers).not.toHaveBeenCalled();
  });
});
