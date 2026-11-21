import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { ApiError } from '../../../../api/ApiError';
import { ACTIVE, AUDITOR, ORGANIZATION_ID, RELEASE_MANAGER } from '../fixtures';
import { rolesQueryKeys } from '../queryKeys';
import { RoleRefusal } from '../RoleRefusal';
import { Permission, RoleCode, RoleScope } from '../schemas';
import type { RolesResponse } from '../types';

const list = mock();
const create = mock();
const update = mock();
const remove = mock();

mock.module('../api', () => ({ rolesApi: { list, create, update, remove } }));

let hooks: typeof import('./index');

beforeAll(async () => {
  hooks = await import('./index');
});

afterAll(() => {
  mock.restore();
});

function client(): QueryClient {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity }, mutations: { retry: false } },
  });
}

function wrapper(queryClient: QueryClient) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
}

function seeded(): QueryClient {
  const queryClient = client();
  queryClient.setQueryData(rolesQueryKeys.organization(ORGANIZATION_ID), ACTIVE);
  return queryClient;
}

function cached(queryClient: QueryClient): RolesResponse | undefined {
  return queryClient.getQueryData(rolesQueryKeys.organization(ORGANIZATION_ID));
}

beforeEach(() => {
  for (const call of [list, create, update, remove]) {
    call.mockReset();
  }
});

describe('useRoles', () => {
  it('loads the roles of the organization', async () => {
    list.mockResolvedValue(ACTIVE);
    const { result } = renderHook(() => hooks.useRoles(ORGANIZATION_ID), {
      wrapper: wrapper(client()),
    });

    await waitFor(() => expect(result.current.response).toEqual(ACTIVE));
    expect(list).toHaveBeenCalledWith(ORGANIZATION_ID);
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it('waits for an organization before loading', () => {
    const { result } = renderHook(() => hooks.useRoles(null), { wrapper: wrapper(client()) });

    expect(result.current.response).toBeNull();
    expect(list).not.toHaveBeenCalled();
  });

  it('reports a refused load without retrying it', async () => {
    list.mockRejectedValue(new ApiError('Failed to load the roles: Not found', 404));
    const { result } = renderHook(() => hooks.useRoles(ORGANIZATION_ID), {
      wrapper: wrapper(client()),
    });

    await waitFor(() => expect(result.current.error).toBe('Failed to load the roles: Not found'));
    expect(list).toHaveBeenCalledTimes(1);
  });

  it('loads again on refresh', async () => {
    list.mockResolvedValue(ACTIVE);
    const { result } = renderHook(() => hooks.useRoles(ORGANIZATION_ID), {
      wrapper: wrapper(client()),
    });
    await waitFor(() => expect(result.current.response).toEqual(ACTIVE));

    await act(() => result.current.refresh());

    expect(list).toHaveBeenCalledTimes(2);
  });
});

describe('useCreateRole', () => {
  it('adds the created role to the cached list', async () => {
    const created = { ...AUDITOR, id: 'new-role', name: 'Reader' };
    create.mockResolvedValue(created);
    list.mockImplementation(() => new Promise(() => {}));
    const queryClient = seeded();
    const done = mock();
    const { result } = renderHook(() => hooks.useCreateRole(ORGANIZATION_ID), {
      wrapper: wrapper(queryClient),
    });
    const request = { name: 'Reader', scope: RoleScope.Organization, permissions: [] };

    act(() => result.current.create(request, done));

    await waitFor(() => expect(done).toHaveBeenCalledWith(created));
    expect(create).toHaveBeenCalledWith(ORGANIZATION_ID, request);
    expect(cached(queryClient)?.roles.map((role) => role.id)).toEqual([
      RELEASE_MANAGER.id,
      AUDITOR.id,
      'new-role',
    ]);
  });

  it('exposes a refusal and keeps the cached list', async () => {
    create.mockRejectedValue(new RoleRefusal({ error: 'Taken', code: RoleCode.NameTaken }, 409));
    list.mockResolvedValue(ACTIVE);
    const queryClient = seeded();
    const { result } = renderHook(() => hooks.useCreateRole(ORGANIZATION_ID), {
      wrapper: wrapper(queryClient),
    });

    act(() =>
      result.current.create({ name: 'Auditor', scope: RoleScope.Organization, permissions: [] })
    );

    await waitFor(() => expect(result.current.refusal?.code).toBe(RoleCode.NameTaken));
    expect(cached(queryClient)?.roles).toEqual(ACTIVE.roles);

    act(() => result.current.reset());
    await waitFor(() => expect(result.current.refusal).toBeNull());
  });
});

describe('useUpdateRole', () => {
  it('replaces the saved role in the cached list', async () => {
    const saved = { ...RELEASE_MANAGER, permissions: [Permission.WorkspaceRead] };
    update.mockResolvedValue(saved);
    list.mockImplementation(() => new Promise(() => {}));
    const queryClient = seeded();
    const done = mock();
    const { result } = renderHook(() => hooks.useUpdateRole(ORGANIZATION_ID), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.update(RELEASE_MANAGER.id, { permissions: [] }, done));

    await waitFor(() => expect(done).toHaveBeenCalledWith(saved));
    expect(update).toHaveBeenCalledWith(ORGANIZATION_ID, RELEASE_MANAGER.id, { permissions: [] });
    expect(cached(queryClient)?.roles).toEqual([saved, AUDITOR]);
  });

  it('exposes a widening refusal', async () => {
    update.mockRejectedValue(new RoleRefusal({ error: 'Exceeds', code: RoleCode.Exceeds }, 403));
    list.mockResolvedValue(ACTIVE);
    const { result } = renderHook(() => hooks.useUpdateRole(ORGANIZATION_ID), {
      wrapper: wrapper(seeded()),
    });

    act(() => result.current.update(RELEASE_MANAGER.id, { permissions: [] }));

    await waitFor(() => expect(result.current.refusal?.code).toBe(RoleCode.Exceeds));
  });
});

describe('useDeleteRole', () => {
  it('drops the deleted role from the cached list', async () => {
    remove.mockResolvedValue(undefined);
    list.mockImplementation(() => new Promise(() => {}));
    const queryClient = seeded();
    const done = mock();
    const { result } = renderHook(() => hooks.useDeleteRole(ORGANIZATION_ID), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.remove(AUDITOR.id, done));

    await waitFor(() => expect(done).toHaveBeenCalled());
    expect(remove).toHaveBeenCalledWith(ORGANIZATION_ID, AUDITOR.id);
    expect(cached(queryClient)?.roles).toEqual([RELEASE_MANAGER]);
  });

  it('exposes how many members still hold a role in use', async () => {
    remove.mockRejectedValue(
      new RoleRefusal({ error: 'In use', code: RoleCode.InUse, members: 3 }, 409)
    );
    list.mockResolvedValue(ACTIVE);
    const queryClient = seeded();
    const { result } = renderHook(() => hooks.useDeleteRole(ORGANIZATION_ID), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.remove(RELEASE_MANAGER.id));

    await waitFor(() => expect(result.current.refusal?.members).toBe(3));
    expect(cached(queryClient)?.roles).toEqual(ACTIVE.roles);
  });
});
