import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { ApiError } from '../../../../api/ApiError';
import {
  ACTIVE_ORGANIZATION,
  ACTIVE_WORKSPACE_INHERITED,
  ORGANIZATION_ID,
  WORKSPACE_ID,
} from '../fixtures';
import { PolicyRefusal } from '../PolicyRefusal';
import { policyQueryKeys } from '../queryKeys';
import { PolicyCode } from '../schemas';

const organization = mock();
const updateOrganization = mock();

mock.module('../api', () => ({
  policyApi: { organization, updateOrganization, workspace: mock(), updateWorkspace: mock() },
}));

let useOrganizationPolicy: typeof import('./useOrganizationPolicy').useOrganizationPolicy;
let useUpdateOrganizationPolicy: typeof import('./useUpdateOrganizationPolicy').useUpdateOrganizationPolicy;

beforeAll(async () => {
  ({ useOrganizationPolicy } = await import('./useOrganizationPolicy'));
  ({ useUpdateOrganizationPolicy } = await import('./useUpdateOrganizationPolicy'));
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

describe('useOrganizationPolicy', () => {
  beforeEach(() => {
    organization.mockReset();
  });

  it('loads the policy of the organization', async () => {
    organization.mockResolvedValue(ACTIVE_ORGANIZATION);
    const { result } = renderHook(() => useOrganizationPolicy(ORGANIZATION_ID), {
      wrapper: wrapper(client()),
    });

    await waitFor(() => expect(result.current.policy).toEqual(ACTIVE_ORGANIZATION));
    expect(organization).toHaveBeenCalledWith(ORGANIZATION_ID);
    expect(result.current.error).toBeNull();
  });

  it('waits for an organization before loading', () => {
    const { result } = renderHook(() => useOrganizationPolicy(null), {
      wrapper: wrapper(client()),
    });

    expect(result.current.policy).toBeUndefined();
    expect(organization).not.toHaveBeenCalled();
  });

  it('reports a refused load without retrying it', async () => {
    organization.mockRejectedValue(new ApiError('Failed to load the agent policy: Forbidden', 403));
    const { result } = renderHook(() => useOrganizationPolicy(ORGANIZATION_ID), {
      wrapper: wrapper(client()),
    });

    await waitFor(() =>
      expect(result.current.error).toBe('Failed to load the agent policy: Forbidden')
    );
    expect(organization).toHaveBeenCalledTimes(1);
  });
});

describe('useUpdateOrganizationPolicy', () => {
  beforeEach(() => {
    updateOrganization.mockReset();
  });

  it('saves the patch, caches the result and refreshes the workspaces below it', async () => {
    const saved = {
      ...ACTIVE_ORGANIZATION,
      policy: { ...ACTIVE_ORGANIZATION.policy, domains: null },
    };
    updateOrganization.mockResolvedValue(saved);
    const queryClient = client();
    queryClient.setQueryData(policyQueryKeys.organization(ORGANIZATION_ID), ACTIVE_ORGANIZATION);
    queryClient.setQueryData(
      policyQueryKeys.workspace(ORGANIZATION_ID, WORKSPACE_ID),
      ACTIVE_WORKSPACE_INHERITED
    );
    const updated = mock();
    const { result } = renderHook(() => useUpdateOrganizationPolicy(ORGANIZATION_ID), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.update({ domains: null }, updated));

    await waitFor(() => expect(updated).toHaveBeenCalledWith(saved));
    expect(updateOrganization).toHaveBeenCalledWith(ORGANIZATION_ID, { domains: null });
    expect(queryClient.getQueryData(policyQueryKeys.organization(ORGANIZATION_ID))).toEqual(saved);
    expect(
      queryClient.getQueryState(policyQueryKeys.workspace(ORGANIZATION_ID, WORKSPACE_ID))
        ?.isInvalidated
    ).toBe(true);
  });

  it('exposes a refusal and keeps the cached policy', async () => {
    updateOrganization.mockRejectedValue(
      new PolicyRefusal({ error: 'License required', code: PolicyCode.LicenseRequired }, 403)
    );
    const queryClient = client();
    queryClient.setQueryData(policyQueryKeys.organization(ORGANIZATION_ID), ACTIVE_ORGANIZATION);
    const { result } = renderHook(() => useUpdateOrganizationPolicy(ORGANIZATION_ID), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.update({ models: null }));

    await waitFor(() => expect(result.current.refusal?.code).toBe(PolicyCode.LicenseRequired));
    expect(queryClient.getQueryData(policyQueryKeys.organization(ORGANIZATION_ID))).toEqual(
      ACTIVE_ORGANIZATION
    );

    act(() => result.current.reset());
    await waitFor(() => expect(result.current.refusal).toBeNull());
  });
});
