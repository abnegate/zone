import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import {
  ACTIVE_WORKSPACE_INHERITED,
  ACTIVE_WORKSPACE_OVERRIDDEN,
  ORGANIZATION_ID,
  WORKSPACE_ID,
} from '../fixtures';
import { PolicyRefusal } from '../PolicyRefusal';
import { policyQueryKeys } from '../queryKeys';
import { PolicyCode } from '../schemas';

const workspace = mock();
const updateWorkspace = mock();

mock.module('../api', () => ({
  policyApi: { organization: mock(), updateOrganization: mock(), workspace, updateWorkspace },
}));

let useWorkspacePolicy: typeof import('./useWorkspacePolicy').useWorkspacePolicy;
let useUpdateWorkspacePolicy: typeof import('./useUpdateWorkspacePolicy').useUpdateWorkspacePolicy;

beforeAll(async () => {
  ({ useWorkspacePolicy } = await import('./useWorkspacePolicy'));
  ({ useUpdateWorkspacePolicy } = await import('./useUpdateWorkspacePolicy'));
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

describe('useWorkspacePolicy', () => {
  beforeEach(() => {
    workspace.mockReset();
  });

  it('loads the layered policy of the workspace', async () => {
    workspace.mockResolvedValue(ACTIVE_WORKSPACE_OVERRIDDEN);
    const { result } = renderHook(() => useWorkspacePolicy(ORGANIZATION_ID, WORKSPACE_ID), {
      wrapper: wrapper(client()),
    });

    await waitFor(() => expect(result.current.policy).toEqual(ACTIVE_WORKSPACE_OVERRIDDEN));
    expect(workspace).toHaveBeenCalledWith(ORGANIZATION_ID, WORKSPACE_ID);
  });

  it('waits for both an organization and a workspace', () => {
    renderHook(() => useWorkspacePolicy(ORGANIZATION_ID, null), { wrapper: wrapper(client()) });
    renderHook(() => useWorkspacePolicy(null, WORKSPACE_ID), { wrapper: wrapper(client()) });

    expect(workspace).not.toHaveBeenCalled();
  });
});

describe('useUpdateWorkspacePolicy', () => {
  beforeEach(() => {
    updateWorkspace.mockReset();
  });

  it('saves the patch and caches the layered result', async () => {
    updateWorkspace.mockResolvedValue(ACTIVE_WORKSPACE_INHERITED);
    const queryClient = client();
    const key = policyQueryKeys.workspace(ORGANIZATION_ID, WORKSPACE_ID);
    queryClient.setQueryData(key, ACTIVE_WORKSPACE_OVERRIDDEN);
    const updated = mock();
    const { result } = renderHook(() => useUpdateWorkspacePolicy(ORGANIZATION_ID, WORKSPACE_ID), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.update({ providers: null }, updated));

    await waitFor(() => expect(updated).toHaveBeenCalledWith(ACTIVE_WORKSPACE_INHERITED));
    expect(updateWorkspace).toHaveBeenCalledWith(ORGANIZATION_ID, WORKSPACE_ID, {
      providers: null,
    });
    expect(queryClient.getQueryData(key)).toEqual(ACTIVE_WORKSPACE_INHERITED);
  });

  it('exposes a widening refusal', async () => {
    updateWorkspace.mockRejectedValue(
      new PolicyRefusal({ error: 'Wider', code: PolicyCode.Wider }, 400)
    );
    const { result } = renderHook(() => useUpdateWorkspacePolicy(ORGANIZATION_ID, WORKSPACE_ID), {
      wrapper: wrapper(client()),
    });

    act(() => result.current.update({ providers: ['openai'] }));

    await waitFor(() => expect(result.current.refusal?.code).toBe(PolicyCode.Wider));
    expect(result.current.error).toBe('Wider');
  });
});

describe('policyQueryKeys', () => {
  it('keys the policies under agent-policy', () => {
    expect(policyQueryKeys.organization('o')).toEqual(['agent-policy', 'organization', 'o']);
    expect(policyQueryKeys.workspace('o', 'w')).toEqual(['agent-policy', 'workspace', 'o', 'w']);
  });
});
