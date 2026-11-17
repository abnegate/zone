import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { TokenTotals } from '../types';

const mockGetChatTokenTotals = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: { getChatTokenTotals: mockGetChatTokenTotals },
}));

let useChatTokenUsage: typeof import('./useChatTokenUsage').useChatTokenUsage;
let RECORDING_SETTLE_DELAY: number;

beforeAll(async () => {
  ({ useChatTokenUsage } = await import('./useChatTokenUsage'));
  ({ RECORDING_SETTLE_DELAY } = await import('./useChatPullRequests'));
});

afterAll(() => {
  mock.restore();
});

const TOTALS: TokenTotals = {
  turns: 3,
  reported_turns: 3,
  prompt_tokens: 29_000,
  completion_tokens: 1_000,
  total_tokens: 30_000,
  rounds: 7,
  tool_calls: 4,
  peak_context_tokens: 12_000,
  instructions_tokens: 15_000,
  conversation_tokens: 9_000,
  tools_tokens: 4_000,
  results_tokens: 1_500,
  summary_tokens: 0,
  attachments_tokens: 0,
  overhead_tokens: 500,
  first_at: '2026-11-16T00:00:00Z',
  last_at: '2026-11-16T01:00:00Z',
};

function totals(total: number): TokenTotals {
  return { ...TOTALS, total_tokens: total };
}

describe('useChatTokenUsage', () => {
  beforeEach(() => {
    mockGetChatTokenTotals.mockReset();
  });

  it('reads the totals of the chat it was given', async () => {
    mockGetChatTokenTotals.mockResolvedValue(totals(30_000));

    const { result } = renderHook(() => useChatTokenUsage('chat-1', false));

    await waitFor(() => expect(result.current?.total_tokens).toBe(30_000));
    expect(mockGetChatTokenTotals).toHaveBeenCalledWith('chat-1');
  });

  it('reads nothing and reports nothing without a chat', async () => {
    const { result } = renderHook(() => useChatTokenUsage(null, false));

    await waitFor(() => expect(result.current).toBeNull());
    expect(mockGetChatTokenTotals).not.toHaveBeenCalled();
  });

  it('does not show one chat total against another while the new read is in flight', async () => {
    mockGetChatTokenTotals.mockImplementation(async (chatId: string) =>
      totals(chatId === 'chat-1' ? 30_000 : 90_000)
    );

    const { result, rerender } = renderHook(({ chatId }) => useChatTokenUsage(chatId, false), {
      initialProps: { chatId: 'chat-1' as string | null },
    });
    await waitFor(() => expect(result.current?.total_tokens).toBe(30_000));

    rerender({ chatId: 'chat-2' });
    expect(result.current).toBeNull();
    await waitFor(() => expect(result.current?.total_tokens).toBe(90_000));
  });

  it('reads again once the turn that just ended has had time to be recorded', async () => {
    mockGetChatTokenTotals.mockResolvedValue(totals(30_000));
    const { result, rerender } = renderHook(
      ({ streaming }) => useChatTokenUsage('chat-1', streaming),
      { initialProps: { streaming: true } }
    );
    await waitFor(() => expect(result.current?.total_tokens).toBe(30_000));

    mockGetChatTokenTotals.mockResolvedValue(totals(45_000));
    rerender({ streaming: false });

    await waitFor(() => expect(result.current?.total_tokens).toBe(45_000), {
      timeout: RECORDING_SETTLE_DELAY + 2_000,
    });
    expect(mockGetChatTokenTotals).toHaveBeenCalledTimes(2);
  });

  it('keeps the last totals when a read fails', async () => {
    mockGetChatTokenTotals.mockResolvedValue(totals(30_000));
    const { result, rerender } = renderHook(
      ({ streaming }) => useChatTokenUsage('chat-1', streaming),
      { initialProps: { streaming: true } }
    );
    await waitFor(() => expect(result.current?.total_tokens).toBe(30_000));

    mockGetChatTokenTotals.mockRejectedValue(new Error('Failed to load token usage'));
    await act(async () => rerender({ streaming: false }));

    await waitFor(() => expect(mockGetChatTokenTotals).toHaveBeenCalledTimes(2), {
      timeout: RECORDING_SETTLE_DELAY + 2_000,
    });
    expect(result.current?.total_tokens).toBe(30_000);
  });
});
