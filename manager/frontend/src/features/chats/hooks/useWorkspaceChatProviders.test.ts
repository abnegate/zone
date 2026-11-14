import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ChatProviderOffer, WorkspaceChatProviders } from '../types';

const mockGetWorkspaceChatProviders = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: {
    getWorkspaceChatProviders: mockGetWorkspaceChatProviders,
  },
}));

let useWorkspaceChatProviders: typeof import('./useWorkspaceChatProviders').useWorkspaceChatProviders;

beforeAll(async () => {
  ({ useWorkspaceChatProviders } = await import('./useWorkspaceChatProviders'));
});

afterAll(() => {
  mock.restore();
});

const claude: ChatProviderOffer = {
  provider: 'claude_code',
  label: 'Claude Code',
  usable: true,
  reason: null,
  models: [{ name: 'sonnet', label: 'Sonnet', efforts: ['low', 'medium', 'high', 'xhigh', 'max'] }],
  efforts: ['low', 'medium', 'high', 'xhigh', 'max'],
  requires_model: false,
};

const selfHosted: ChatProviderOffer = {
  provider: 'self_hosted',
  label: 'Self-Hosted',
  usable: true,
  reason: null,
  models: [],
  efforts: ['off', 'low', 'medium', 'high'],
  requires_model: false,
};

const offers = (offer: ChatProviderOffer): WorkspaceChatProviders => ({
  default: offer,
  providers: [selfHosted, claude],
});

describe('useWorkspaceChatProviders', () => {
  beforeEach(() => {
    mockGetWorkspaceChatProviders.mockReset();
  });

  it('loads the offers of the current workspace and reloads when it changes', async () => {
    mockGetWorkspaceChatProviders.mockImplementation(async (workspaceId: string) =>
      offers(workspaceId === 'ws-1' ? claude : selfHosted)
    );

    const { result, rerender } = renderHook(
      ({ workspaceId }) => useWorkspaceChatProviders(workspaceId),
      { initialProps: { workspaceId: 'ws-1' as string | null } }
    );

    await waitFor(() => {
      expect(result.current.providers?.default.label).toBe('Claude Code');
    });
    expect(result.current.providers?.default.models.map((model) => model.label)).toEqual([
      'Sonnet',
    ]);

    rerender({ workspaceId: 'ws-2' });
    await waitFor(() => {
      expect(result.current.providers?.default.label).toBe('Self-Hosted');
    });
    expect(mockGetWorkspaceChatProviders).toHaveBeenLastCalledWith('ws-2');

    rerender({ workspaceId: null });
    await waitFor(() => {
      expect(result.current.providers).toBeNull();
    });
    expect(mockGetWorkspaceChatProviders).toHaveBeenCalledTimes(2);
  });

  it("never reports the previous workspace's offers for the next one", async () => {
    mockGetWorkspaceChatProviders.mockImplementation((workspaceId: string) =>
      workspaceId === 'ws-1' ? Promise.resolve(offers(claude)) : new Promise(() => {})
    );
    const seen: Array<string | undefined> = [];

    const { result, rerender } = renderHook(
      ({ workspaceId }) => {
        const loaded = useWorkspaceChatProviders(workspaceId);
        seen.push(`${workspaceId}:${loaded.providers?.default.label ?? 'none'}`);
        return loaded;
      },
      { initialProps: { workspaceId: 'ws-1' as string | null } }
    );
    await waitFor(() => {
      expect(result.current.providers?.default.label).toBe('Claude Code');
    });
    seen.length = 0;

    rerender({ workspaceId: 'ws-2' });

    expect(seen).not.toContain('ws-2:Claude Code');
    expect(result.current.providers).toBeNull();
  });

  it('drops a response that arrives after the workspace changed', async () => {
    let resolve: (value: WorkspaceChatProviders) => void = () => {};
    const first = new Promise<WorkspaceChatProviders>((done) => {
      resolve = done;
    });
    mockGetWorkspaceChatProviders.mockImplementation((workspaceId: string) =>
      workspaceId === 'ws-1' ? first : Promise.resolve(offers(selfHosted))
    );

    const { result, rerender } = renderHook(
      ({ workspaceId }) => useWorkspaceChatProviders(workspaceId),
      { initialProps: { workspaceId: 'ws-1' as string | null } }
    );
    rerender({ workspaceId: 'ws-2' });
    await waitFor(() => {
      expect(result.current.providers?.default.label).toBe('Self-Hosted');
    });

    await act(async () => {
      resolve(offers(claude));
      await first;
    });

    expect(result.current.providers?.default.label).toBe('Self-Hosted');
    expect(result.current.loading).toBe(false);
  });

  it('leaves the offers empty and names the reason when a load fails', async () => {
    mockGetWorkspaceChatProviders.mockRejectedValue(
      new Error('Failed to load AI providers: Forbidden')
    );

    const { result } = renderHook(() => useWorkspaceChatProviders('ws-1'));

    await waitFor(() => {
      expect(result.current.error).toBe('Failed to load AI providers: Forbidden');
    });
    expect(result.current.providers).toBeNull();
    expect(result.current.loading).toBe(false);
  });

  it('reloads on refresh', async () => {
    mockGetWorkspaceChatProviders.mockResolvedValueOnce(offers(selfHosted));
    mockGetWorkspaceChatProviders.mockResolvedValueOnce(offers(claude));

    const { result } = renderHook(() => useWorkspaceChatProviders('ws-1'));
    await waitFor(() => {
      expect(result.current.providers?.default.label).toBe('Self-Hosted');
    });

    await act(async () => {
      await result.current.refresh();
    });

    expect(result.current.providers?.default.label).toBe('Claude Code');
    expect(mockGetWorkspaceChatProviders).toHaveBeenCalledTimes(2);
  });

  it('keeps the last offers and names the reason when a refresh fails', async () => {
    mockGetWorkspaceChatProviders.mockResolvedValueOnce(offers(selfHosted));
    mockGetWorkspaceChatProviders.mockRejectedValueOnce(
      new Error('Failed to load AI providers: 502')
    );

    const { result } = renderHook(() => useWorkspaceChatProviders('ws-1'));
    await waitFor(() => {
      expect(result.current.providers?.default.label).toBe('Self-Hosted');
    });

    await act(async () => {
      await result.current.refresh();
    });

    expect(result.current.providers?.default.label).toBe('Self-Hosted');
    expect(result.current.error).toBe('Failed to load AI providers: 502');
  });
});
