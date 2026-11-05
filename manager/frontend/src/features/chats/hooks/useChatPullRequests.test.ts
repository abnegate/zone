import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ChatPullRequest } from '../types';

const mockGetChatPullRequests = mock();
const mockUpdateChatPullRequest = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: {
    getChatPullRequests: mockGetChatPullRequests,
    updateChatPullRequest: mockUpdateChatPullRequest,
  },
}));

let useChatPullRequests: typeof import('./useChatPullRequests').useChatPullRequests;
let RECORDING_SETTLE_DELAY: number;

beforeAll(async () => {
  ({ useChatPullRequests, RECORDING_SETTLE_DELAY } = await import('./useChatPullRequests'));
});

afterAll(() => {
  mock.restore();
});

const OPEN: ChatPullRequest = {
  id: 'pr-1',
  provider: 'github',
  owner: 'acme',
  repo: 'widgets',
  number: 12,
  url: 'https://github.com/acme/widgets/pull/12',
  title: 'Add widgets',
  dismissed: false,
  created_at: '2026-11-05T10:00:00Z',
  status: null,
  writable: true,
  credentialed: true,
};

describe('useChatPullRequests', () => {
  beforeEach(() => {
    mockGetChatPullRequests.mockReset();
    mockUpdateChatPullRequest.mockReset();
  });

  it('keeps a dismissal when a read that started before it answers after it', async () => {
    mockGetChatPullRequests.mockResolvedValueOnce([OPEN]);
    const { result } = renderHook(() => useChatPullRequests('chat-1', false));
    await waitFor(() => expect(result.current.pullRequests).toEqual([OPEN]));

    let answerStaleRead: (rows: ChatPullRequest[]) => void = () => undefined;
    mockGetChatPullRequests.mockImplementationOnce(
      () => new Promise((resolve) => (answerStaleRead = resolve))
    );
    mockUpdateChatPullRequest.mockResolvedValueOnce({ ...OPEN, dismissed: true });

    let reading: Promise<void> = Promise.resolve();
    act(() => {
      reading = result.current.refresh();
    });
    await act(() => result.current.change(OPEN, { dismissed: true }));
    await act(async () => {
      answerStaleRead([OPEN]);
      await reading;
    });

    expect(result.current.pullRequests).toEqual([]);
    expect(result.current.dismissed?.id).toBe('pr-1');
  });

  it('reads again as a turn ends, and once more after the turn has recorded what it opened', async () => {
    mockGetChatPullRequests.mockResolvedValue([]);
    const { rerender } = renderHook(({ streaming }) => useChatPullRequests('chat-1', streaming), {
      initialProps: { streaming: true },
    });
    await waitFor(() => expect(mockGetChatPullRequests).toHaveBeenCalledTimes(1));

    mockGetChatPullRequests.mockResolvedValue([OPEN]);
    rerender({ streaming: false });

    await waitFor(() => expect(mockGetChatPullRequests).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(mockGetChatPullRequests).toHaveBeenCalledTimes(3), {
      timeout: RECORDING_SETTLE_DELAY + 2_000,
    });
  });
});
