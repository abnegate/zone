import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { FleetRefusal } from '../FleetRefusal';
import {
  ACTIVE_POLICY,
  ACTIVE_RUNNERS,
  CURRENT_RUNNER,
  NO_RUNNER_LEFT,
  OFFLINE_RUNNER,
  ORGANIZATION_ID,
  ORGANIZATION_LABELS,
  WORKSPACE_ID,
  WORKSPACE_LABELS,
} from '../fixtures';
import {
  fromOrganization,
  fromWorkspace,
  organizationPlacement,
  workspacePlacement,
} from '../placement';
import { fleetQueryKeys } from '../queryKeys';
import { RunnerState } from '../schemas';

const updatePolicy = mock();
const updateRunner = mock();
const removeRunner = mock();
const labels = mock();
const updateLabels = mock();

mock.module('../api', () => ({
  fleetApi: {
    policy: mock(),
    updatePolicy,
    runners: mock(),
    updateRunner,
    removeRunner,
    labels,
    updateLabels,
  },
}));

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

function invalidated(queryClient: QueryClient, key: readonly unknown[]): boolean | undefined {
  return queryClient.getQueryState(key)?.isInvalidated;
}

const ORGANIZATION = organizationPlacement(ORGANIZATION_ID);

const WORKSPACE = workspacePlacement(ORGANIZATION_ID, WORKSPACE_ID);

describe('fleet hooks', () => {
  beforeEach(() => {
    for (const call of [updatePolicy, updateRunner, removeRunner, labels, updateLabels]) {
      call.mockReset();
    }
  });

  it('caches a saved minimum and refreshes the runners and label counts it changes', async () => {
    updatePolicy.mockResolvedValue(ACTIVE_POLICY);
    const queryClient = client();
    queryClient.setQueryData(fleetQueryKeys.runners, ACTIVE_RUNNERS);
    queryClient.setQueryData(
      fleetQueryKeys.placement(ORGANIZATION),
      fromOrganization(ORGANIZATION_LABELS)
    );
    const { result } = renderHook(() => hooks.useUpdateFleetPolicy(), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.update({ minimum_version: '1.4.0' }));

    await waitFor(() =>
      expect(queryClient.getQueryData(fleetQueryKeys.policy)).toEqual(ACTIVE_POLICY)
    );
    expect(invalidated(queryClient, fleetQueryKeys.runners)).toBe(true);
    expect(invalidated(queryClient, fleetQueryKeys.placement(ORGANIZATION))).toBe(true);
  });

  it('hands a lockout refusal to the caller so it can ask to confirm', async () => {
    updateRunner.mockRejectedValue(new FleetRefusal(NO_RUNNER_LEFT, 409));
    const refused = mock();
    const { result } = renderHook(() => hooks.useUpdateRunner(), { wrapper: wrapper(client()) });

    act(() =>
      result.current.update(CURRENT_RUNNER.id, { state: RunnerState.Revoked }, undefined, refused)
    );

    await waitFor(() => expect(refused).toHaveBeenCalledTimes(1));
    expect((refused.mock.calls[0][0] as FleetRefusal).leavesNoRunner).toBe(true);
    expect(result.current.refusal?.leavesNoRunner).toBe(true);
  });

  it('puts a changed runner straight into the list', async () => {
    const cordoned = { ...CURRENT_RUNNER, state: RunnerState.Cordoned, serving: false };
    updateRunner.mockResolvedValue(cordoned);
    const queryClient = client();
    queryClient.setQueryData(fleetQueryKeys.runners, ACTIVE_RUNNERS);
    const { result } = renderHook(() => hooks.useUpdateRunner(), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.update(CURRENT_RUNNER.id, { state: RunnerState.Cordoned }));

    await waitFor(() =>
      expect(
        queryClient.getQueryData<typeof ACTIVE_RUNNERS>(fleetQueryKeys.runners)?.runners[0]
      ).toEqual(cordoned)
    );
  });

  it('drops a removed runner from the list', async () => {
    removeRunner.mockResolvedValue(undefined);
    const queryClient = client();
    queryClient.setQueryData(fleetQueryKeys.runners, ACTIVE_RUNNERS);
    const { result } = renderHook(() => hooks.useRemoveRunner(), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.remove(OFFLINE_RUNNER.id));

    await waitFor(() =>
      expect(
        queryClient
          .getQueryData<typeof ACTIVE_RUNNERS>(fleetQueryKeys.runners)
          ?.runners.map((runner) => runner.id)
      ).not.toContain(OFFLINE_RUNNER.id)
    );
  });

  it('loads the labels of a scope', async () => {
    labels.mockResolvedValue(fromWorkspace(WORKSPACE_LABELS));
    const { result } = renderHook(() => hooks.useRunnerLabels(WORKSPACE), {
      wrapper: wrapper(client()),
    });

    await waitFor(() => expect(result.current.placement).toEqual(fromWorkspace(WORKSPACE_LABELS)));
    expect(labels).toHaveBeenCalledWith(WORKSPACE);
  });

  it('refreshes the workspaces of an organization whose labels changed', async () => {
    const saved = fromOrganization({ ...ORGANIZATION_LABELS, labels: ['gpu'] });
    updateLabels.mockResolvedValue(saved);
    const queryClient = client();
    queryClient.setQueryData(fleetQueryKeys.placement(WORKSPACE), fromWorkspace(WORKSPACE_LABELS));
    const { result } = renderHook(() => hooks.useUpdateRunnerLabels(ORGANIZATION), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.update(['gpu']));

    await waitFor(() =>
      expect(queryClient.getQueryData(fleetQueryKeys.placement(ORGANIZATION))).toEqual(saved)
    );
    expect(updateLabels).toHaveBeenCalledWith(ORGANIZATION, { labels: ['gpu'] });
    expect(invalidated(queryClient, fleetQueryKeys.placement(WORKSPACE))).toBe(true);
  });
});
