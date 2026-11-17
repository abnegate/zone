import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  jest,
  mock,
} from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { TokenTotals } from '../types';

const mockGetChatTokenTotals = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: { getChatTokenTotals: mockGetChatTokenTotals },
}));

let useChatTokenUsage: typeof import('./useChatTokenUsage').useChatTokenUsage;
let RECORDING_SETTLE_DELAY: number;
let LATE_RECORDING_DELAY: number;

beforeAll(async () => {
  ({ useChatTokenUsage, LATE_RECORDING_DELAY } = await import('./useChatTokenUsage'));
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

  it('reads a chat reopened before its last turn was recorded once that turn lands', async () => {
    let ended = Number.POSITIVE_INFINITY;
    mockGetChatTokenTotals.mockImplementation(async (chatId: string) => {
      if (chatId === 'chat-2') return totals(90_000);
      const recorded = Date.now() - ended >= RECORDING_SETTLE_DELAY / 2;
      return totals(recorded ? 45_000 : 30_000);
    });
    const { result, rerender } = renderHook(
      ({ chatId, streaming }) => useChatTokenUsage(chatId, streaming),
      { initialProps: { chatId: 'chat-1', streaming: true } }
    );
    await waitFor(() => expect(result.current?.total_tokens).toBe(30_000));

    ended = Date.now();
    rerender({ chatId: 'chat-1', streaming: false });
    rerender({ chatId: 'chat-2', streaming: false });
    await waitFor(() => expect(result.current?.total_tokens).toBe(90_000));
    rerender({ chatId: 'chat-1', streaming: false });
    await waitFor(() => expect(result.current?.total_tokens).toBe(30_000));

    await waitFor(() => expect(result.current?.total_tokens).toBe(45_000), {
      timeout: RECORDING_SETTLE_DELAY + 2_000,
    });
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

  describe('with the clock under test control', () => {
    beforeEach(() => {
      jest.useFakeTimers();
    });

    afterEach(() => {
      jest.useRealTimers();
    });

    async function advance(milliseconds: number): Promise<void> {
      await act(async () => {
        jest.advanceTimersByTime(milliseconds);
        await Promise.resolve();
        await Promise.resolve();
      });
    }

    function readsOf(chatId: string): number {
      return mockGetChatTokenTotals.mock.calls.filter(([read]) => read === chatId).length;
    }

    it('reads once more later, so a slow write still shows', async () => {
      mockGetChatTokenTotals.mockResolvedValue(totals(30_000));
      const { result, rerender } = renderHook(
        ({ streaming }) => useChatTokenUsage('chat-1', streaming),
        { initialProps: { streaming: true } }
      );
      await advance(0);
      rerender({ streaming: false });

      await advance(RECORDING_SETTLE_DELAY);
      expect(readsOf('chat-1')).toBe(2);
      expect(result.current?.total_tokens).toBe(30_000);

      mockGetChatTokenTotals.mockResolvedValue(totals(45_000));
      await advance(LATE_RECORDING_DELAY - RECORDING_SETTLE_DELAY);

      expect(readsOf('chat-1')).toBe(3);
      expect(result.current?.total_tokens).toBe(45_000);
      await advance(LATE_RECORDING_DELAY);
      expect(readsOf('chat-1')).toBe(3);
    });

    it('stamps the end of a stream on the chat that was streaming, not the one opened next', async () => {
      mockGetChatTokenTotals.mockResolvedValue(totals(30_000));
      const { rerender } = renderHook(
        ({ chatId, streaming }) => useChatTokenUsage(chatId, streaming),
        { initialProps: { chatId: 'chat-1', streaming: true } }
      );
      await advance(0);

      rerender({ chatId: 'chat-2', streaming: false });
      await advance(0);
      await advance(LATE_RECORDING_DELAY * 2);
      expect(readsOf('chat-2')).toBe(1);
    });

    it('reads the chat that was streaming when it is reopened before its write has landed', async () => {
      mockGetChatTokenTotals.mockResolvedValue(totals(30_000));
      const { rerender } = renderHook(
        ({ chatId, streaming }) => useChatTokenUsage(chatId, streaming),
        { initialProps: { chatId: 'chat-1', streaming: true } }
      );
      await advance(0);

      rerender({ chatId: 'chat-2', streaming: false });
      await advance(1_000);
      rerender({ chatId: 'chat-1', streaming: false });
      await advance(0);
      expect(readsOf('chat-1')).toBe(2);

      await advance(RECORDING_SETTLE_DELAY - 1_000);
      expect(readsOf('chat-1')).toBe(3);
      expect(readsOf('chat-2')).toBe(1);
    });

    it('does not keep reading a chat whose reads have all been made', async () => {
      mockGetChatTokenTotals.mockResolvedValue(totals(30_000));
      const { rerender } = renderHook(
        ({ chatId, streaming }) => useChatTokenUsage(chatId, streaming),
        { initialProps: { chatId: 'chat-1', streaming: true } }
      );
      await advance(0);
      rerender({ chatId: 'chat-1', streaming: false });
      await advance(LATE_RECORDING_DELAY);
      expect(readsOf('chat-1')).toBe(3);

      rerender({ chatId: 'chat-2', streaming: false });
      await advance(0);
      rerender({ chatId: 'chat-1', streaming: false });
      await advance(0);
      expect(readsOf('chat-1')).toBe(4);

      await advance(LATE_RECORDING_DELAY * 2);
      expect(readsOf('chat-1')).toBe(4);
    });
  });
});
