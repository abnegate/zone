import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import fixture from '../../../../../../runner/zone_server/tests/fixtures/agents.json';
import type { AgentStatus } from './schemas';
import type { Attempt } from './types';

const agentsApi = {
  list: mock(),
  get: mock(),
  start: mock(),
  submitCode: mock(),
  signOut: mock(),
};

mock.module('../../../api/agents', () => ({ agentsApi }));

let useAgentStatuses: typeof import('./useAgentStatuses').useAgentStatuses;

beforeAll(async () => {
  ({ useAgentStatuses } = await import('./useAgentStatuses'));
});

afterAll(() => {
  mock.restore();
});

const [claude, codex] = fixture.agents as AgentStatus[];
const authorization: Attempt = {
  login: {
    agent: 'claude',
    authorize_url: 'https://claude.com/cai/oauth/authorize?code=true&state=fake-state',
    expires_at: '2026-09-23T04:10:00Z',
  },
  scope: 'full',
  spent: false,
};
const device: Attempt = {
  login: {
    agent: 'codex',
    verification_url: 'https://auth.openai.com/codex/device',
    user_code: 'ABCD-EFGHI',
    expires_at: '2026-09-23T04:15:00Z',
  },
  scope: undefined,
  spent: false,
};

describe('useAgentStatuses', () => {
  beforeEach(() => {
    for (const request of Object.values(agentsApi)) request.mockReset();
    agentsApi.list.mockResolvedValue([claude, codex]);
  });

  it('does not ask the server while no agent provider is selected', async () => {
    const { result } = renderHook(() => useAgentStatuses('org-1', false));
    await act(async () => {});
    expect(agentsApi.list).not.toHaveBeenCalled();
    expect(result.current.statuses).toEqual({});
  });

  it('loads both statuses once an agent provider is selected', async () => {
    const { result, rerender } = renderHook(({ enabled }) => useAgentStatuses('org-1', enabled), {
      initialProps: { enabled: false },
    });
    rerender({ enabled: true });
    await waitFor(() => expect(result.current.statuses.claude?.state).toBe('signed_in'));
    expect(result.current.statuses.codex?.state).toBe('pending');
    expect(agentsApi.list).toHaveBeenCalledTimes(1);
    expect(agentsApi.list).toHaveBeenCalledWith('org-1');
  });

  it('replaces one agent when its status changes', async () => {
    const { result } = renderHook(() => useAgentStatuses('org-1', true));
    await waitFor(() => expect(result.current.statuses.codex).toBeDefined());
    const signedIn: AgentStatus = { ...codex, state: 'signed_in', source: 'zone', pending: null };
    act(() => result.current.update(signedIn));
    expect(result.current.statuses.codex).toEqual(signedIn);
    expect(result.current.statuses.claude).toEqual(claude);
  });

  it("holds each agent's sign-in in flight until it is cleared", async () => {
    const { result } = renderHook(() => useAgentStatuses('org-1', true));
    await waitFor(() => expect(result.current.statuses.claude).toBeDefined());
    expect(result.current.attempts).toEqual({});

    act(() => result.current.setAttempt('claude', authorization));
    act(() => result.current.setAttempt('codex', device));
    expect(result.current.attempts).toEqual({ claude: authorization, codex: device });

    act(() => result.current.setAttempt('claude', null));
    expect(result.current.attempts).toEqual({ codex: device });
  });

  it('keeps a sign-in in flight when the statuses load again for the same organization', async () => {
    const { result, rerender } = renderHook(({ enabled }) => useAgentStatuses('org-1', enabled), {
      initialProps: { enabled: true },
    });
    await waitFor(() => expect(result.current.statuses.claude).toBeDefined());
    act(() => result.current.setAttempt('claude', authorization));

    rerender({ enabled: false });
    rerender({ enabled: true });

    await waitFor(() => expect(agentsApi.list).toHaveBeenCalledTimes(2));
    await act(async () => {});
    expect(result.current.attempts).toEqual({ claude: authorization });
  });

  it("never shows one organization's sign-in in flight under another", async () => {
    const { result, rerender } = renderHook(
      ({ organization }) => useAgentStatuses(organization, true),
      { initialProps: { organization: 'org-1' } }
    );
    await waitFor(() => expect(result.current.statuses.claude).toBeDefined());
    act(() => result.current.setAttempt('claude', authorization));

    rerender({ organization: 'org-2' });

    await waitFor(() => expect(agentsApi.list).toHaveBeenLastCalledWith('org-2'));
    await act(async () => {});
    expect(result.current.attempts).toEqual({});
  });

  it('drops a late write for an organization that is no longer the loaded one', async () => {
    const second: AgentStatus = { ...claude, state: 'signed_out', source: null, label: null };
    agentsApi.list.mockResolvedValueOnce([claude, codex]).mockResolvedValueOnce([second, codex]);
    const { result, rerender } = renderHook(
      ({ organization }) => useAgentStatuses(organization, true),
      { initialProps: { organization: 'org-1' } }
    );
    await waitFor(() => expect(result.current.statuses.claude?.state).toBe('signed_in'));
    const late = result.current.update;
    const lateAttempt = result.current.setAttempt;

    rerender({ organization: 'org-2' });
    await waitFor(() => expect(result.current.statuses.claude?.state).toBe('signed_out'));
    act(() => late({ ...codex, state: 'signed_out', pending: null }));
    act(() => lateAttempt('claude', authorization));

    expect(result.current.statuses).toEqual({ claude: second, codex });
    expect(result.current.attempts).toEqual({});
  });

  it('reports a status list that failed to load', async () => {
    agentsApi.list.mockRejectedValueOnce(new Error('Failed to load coding agent sign-ins: 502'));
    const { result } = renderHook(() => useAgentStatuses('org-1', true));
    await waitFor(() =>
      expect(result.current.error).toBe('Failed to load coding agent sign-ins: 502')
    );
    expect(result.current.statuses).toEqual({});
  });

  it("never shows one organization's sign-in under another", async () => {
    let finishFirst!: (statuses: AgentStatus[]) => void;
    agentsApi.list.mockReturnValueOnce(
      new Promise((resolve) => {
        finishFirst = resolve;
      })
    );
    const second: AgentStatus = { ...claude, state: 'signed_out', source: null, label: null };
    agentsApi.list.mockResolvedValueOnce([second, codex]);
    const { result, rerender } = renderHook(
      ({ organization }) => useAgentStatuses(organization, true),
      { initialProps: { organization: 'org-1' } }
    );
    rerender({ organization: 'org-2' });
    await waitFor(() => expect(result.current.statuses.claude?.state).toBe('signed_out'));
    await act(async () => finishFirst([claude, codex]));
    expect(result.current.statuses.claude?.state).toBe('signed_out');
    expect(agentsApi.list).toHaveBeenLastCalledWith('org-2');
  });
});
