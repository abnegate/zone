import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ChatIndicator } from '../types';

const mockGetChatIndicators = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: {
    getChatIndicators: mockGetChatIndicators,
  },
}));

let useChatIndicators: typeof import('./useChatIndicators').useChatIndicators;
let SETTLE_DELAY: number;

beforeAll(async () => {
  ({ useChatIndicators, SETTLE_DELAY } = await import('./useChatIndicators'));
});

afterAll(() => {
  mock.restore();
});

const WRITING: ChatIndicator = {
  chat_id: 'chat-1',
  activity: 'writing',
  pull_request: null,
  pull_requests: 0,
};

describe('useChatIndicators', () => {
  beforeEach(() => {
    mockGetChatIndicators.mockReset();
  });

  it('keys the readings by chat so a row can look its own up', async () => {
    mockGetChatIndicators.mockResolvedValue([WRITING]);
    const { result } = renderHook(() => useChatIndicators('workspace-1', false));

    await waitFor(() => expect(result.current.indicators).toEqual({ 'chat-1': WRITING }));
    expect(mockGetChatIndicators).toHaveBeenCalledWith('workspace-1', false);
  });

  it('asks for nothing without a workspace', async () => {
    const { result } = renderHook(() => useChatIndicators(null, false));

    await waitFor(() => expect(result.current.indicators).toEqual({}));
    expect(mockGetChatIndicators).not.toHaveBeenCalled();
  });

  it('keeps the last readings when a read fails, and says the statuses are stale', async () => {
    mockGetChatIndicators.mockResolvedValueOnce([WRITING]);
    const { result } = renderHook(() => useChatIndicators('workspace-1', false));
    await waitFor(() => expect(result.current.indicators).toEqual({ 'chat-1': WRITING }));

    mockGetChatIndicators.mockRejectedValueOnce(new Error('GitHub is unreachable'));
    await act(() => result.current.refresh());

    await waitFor(() => expect(result.current.error).toBe('GitHub is unreachable'));
    expect(result.current.indicators).toEqual({ 'chat-1': WRITING });
  });

  it('reads again as a turn ends, and once more after the turn has recorded what it opened', async () => {
    mockGetChatIndicators.mockResolvedValue([]);
    const { rerender } = renderHook(
      ({ streaming }) => useChatIndicators('workspace-1', false, streaming),
      { initialProps: { streaming: true } }
    );
    await waitFor(() => expect(mockGetChatIndicators).toHaveBeenCalledTimes(1));

    rerender({ streaming: false });

    await waitFor(() => expect(mockGetChatIndicators).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(mockGetChatIndicators).toHaveBeenCalledTimes(3), {
      timeout: SETTLE_DELAY + 2_000,
    });
  });
});
