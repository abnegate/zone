import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ChatTokenUsage, TokenTotals } from '../types';

const mockGetChatTokenUsage = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: { getChatTokenUsage: mockGetChatTokenUsage },
}));

let useChatTokenUsage: typeof import('./useChatTokenUsage').useChatTokenUsage;
let TOKEN_TURNS: number;

beforeAll(async () => {
  ({ useChatTokenUsage, TOKEN_TURNS } = await import('./useChatTokenUsage'));
});

afterAll(() => {
  mock.restore();
});

const totals: TokenTotals = {
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

function usage(chatId: string, total: number): ChatTokenUsage {
  return {
    chat_id: chatId,
    totals: { ...totals, total_tokens: total },
    turns: [],
    truncated: false,
  };
}

describe('useChatTokenUsage', () => {
  beforeEach(() => {
    mockGetChatTokenUsage.mockReset();
  });

  it('reads the chat it was given, bounded to the turns the panel shows', async () => {
    mockGetChatTokenUsage.mockResolvedValue(usage('chat-1', 30_000));

    const { result } = renderHook(() => useChatTokenUsage('chat-1', false));

    await waitFor(() => expect(result.current.usage?.totals.total_tokens).toBe(30_000));
    expect(mockGetChatTokenUsage).toHaveBeenCalledWith('chat-1', TOKEN_TURNS);
  });

  it('reads nothing and reports nothing without a chat', async () => {
    const { result } = renderHook(() => useChatTokenUsage(null, false));

    await waitFor(() => expect(result.current.usage).toBeNull());
    expect(mockGetChatTokenUsage).not.toHaveBeenCalled();
  });

  it('does not show one chat total against another while the new read is in flight', async () => {
    mockGetChatTokenUsage.mockImplementation(async (chatId: string) =>
      usage(chatId, chatId === 'chat-1' ? 30_000 : 90_000)
    );

    const { result, rerender } = renderHook(({ chatId }) => useChatTokenUsage(chatId, false), {
      initialProps: { chatId: 'chat-1' as string | null },
    });
    await waitFor(() => expect(result.current.usage?.chat_id).toBe('chat-1'));

    rerender({ chatId: 'chat-2' });
    expect(result.current.usage).toBeNull();
    await waitFor(() => expect(result.current.usage?.chat_id).toBe('chat-2'));
    expect(result.current.usage?.totals.total_tokens).toBe(90_000);
  });

  it('reads again once a turn has finished streaming, because the row lands as it closes', async () => {
    mockGetChatTokenUsage.mockResolvedValue(usage('chat-1', 30_000));

    const { result, rerender } = renderHook(
      ({ streaming }) => useChatTokenUsage('chat-1', streaming),
      {
        initialProps: { streaming: true },
      }
    );
    await waitFor(() => expect(result.current.usage?.chat_id).toBe('chat-1'));
    const reads = mockGetChatTokenUsage.mock.calls.length;

    mockGetChatTokenUsage.mockResolvedValue(usage('chat-1', 45_000));
    rerender({ streaming: false });

    await waitFor(() => expect(result.current.usage?.totals.total_tokens).toBe(45_000));
    expect(mockGetChatTokenUsage.mock.calls.length).toBeGreaterThan(reads);
  });

  it('keeps the last total and reports the failure when a read fails', async () => {
    mockGetChatTokenUsage.mockResolvedValue(usage('chat-1', 30_000));

    const { result } = renderHook(() => useChatTokenUsage('chat-1', false));
    await waitFor(() => expect(result.current.usage?.totals.total_tokens).toBe(30_000));

    mockGetChatTokenUsage.mockRejectedValue(new Error('Failed to load token usage'));
    await act(async () => {
      await result.current.refresh();
    });

    await waitFor(() => expect(result.current.error).toBe('Failed to load token usage'));
    expect(result.current.usage?.totals.total_tokens).toBe(30_000);
  });
});
