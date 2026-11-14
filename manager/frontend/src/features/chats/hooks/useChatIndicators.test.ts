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
    const { result } = renderHook(() => useChatIndicators('workspace-1', false, false, 25));

    await waitFor(() => expect(result.current.indicators).toEqual({ 'chat-1': WRITING }));
    expect(mockGetChatIndicators).toHaveBeenCalledWith('workspace-1', false, 25, undefined);
  });

  it('asks for nothing until the list has rows to read for', async () => {
    const { result } = renderHook(() => useChatIndicators('workspace-1', false, false, 0));

    await waitFor(() => expect(result.current.indicators).toEqual({}));
    expect(mockGetChatIndicators).not.toHaveBeenCalled();
    expect(result.current.loaded).toBe(false);
  });

  it('asks for readings over the slice of the list the console holds', async () => {
    mockGetChatIndicators.mockResolvedValue([WRITING]);
    const { rerender } = renderHook(
      ({ listed }) => useChatIndicators('workspace-1', false, false, listed),
      { initialProps: { listed: 25 } }
    );

    await waitFor(() =>
      expect(mockGetChatIndicators).toHaveBeenCalledWith('workspace-1', false, 25, undefined)
    );

    rerender({ listed: 50 });

    await waitFor(() =>
      expect(mockGetChatIndicators).toHaveBeenCalledWith('workspace-1', false, 50, undefined)
    );
  });

  it('reads in the order the list was asked for, and again when that order changes', async () => {
    mockGetChatIndicators.mockResolvedValue([WRITING]);
    const { rerender } = renderHook(
      ({ sort }) => useChatIndicators('workspace-1', false, false, 25, sort),
      { initialProps: { sort: 'title_asc' as const } }
    );

    await waitFor(() =>
      expect(mockGetChatIndicators).toHaveBeenCalledWith('workspace-1', false, 25, 'title_asc')
    );

    rerender({ sort: 'created_desc' as const });

    await waitFor(() =>
      expect(mockGetChatIndicators).toHaveBeenCalledWith('workspace-1', false, 25, 'created_desc')
    );
  });

  it('counts an empty answer and a failed read as readings that landed', async () => {
    mockGetChatIndicators.mockResolvedValueOnce([]);
    const { result } = renderHook(() => useChatIndicators('workspace-1', false, false, 25));
    await waitFor(() => expect(result.current.loaded).toBe(true));
    expect(result.current.indicators).toEqual({});

    mockGetChatIndicators.mockRejectedValueOnce(new Error('GitHub is unreachable'));
    await act(() => result.current.refresh());
    expect(result.current.loaded).toBe(true);
  });

  it('asks for nothing without a workspace', async () => {
    const { result } = renderHook(() => useChatIndicators(null, false));

    await waitFor(() => expect(result.current.indicators).toEqual({}));
    expect(mockGetChatIndicators).not.toHaveBeenCalled();
  });

  it('keeps the last readings when a read fails, and says the statuses are stale', async () => {
    mockGetChatIndicators.mockResolvedValueOnce([WRITING]);
    const { result } = renderHook(() => useChatIndicators('workspace-1', false, false, 25));
    await waitFor(() => expect(result.current.indicators).toEqual({ 'chat-1': WRITING }));

    mockGetChatIndicators.mockRejectedValueOnce(new Error('GitHub is unreachable'));
    await act(() => result.current.refresh());

    await waitFor(() => expect(result.current.error).toBe('GitHub is unreachable'));
    expect(result.current.indicators).toEqual({ 'chat-1': WRITING });
  });

  it('reads again as a turn ends, and once more after the turn has recorded what it opened', async () => {
    mockGetChatIndicators.mockResolvedValue([]);
    const { rerender } = renderHook(
      ({ streaming }) => useChatIndicators('workspace-1', false, streaming, 25),
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
