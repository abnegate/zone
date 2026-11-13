import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { Chat, ChatWithMessages } from '../types';
import { histories } from '../utils/history';

const mockGetChat = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: { getChat: mockGetChat },
}));

let usePreloadChats: typeof import('./usePreloadChats').usePreloadChats;
let PRELOADS_AT_ONCE: number;

beforeAll(async () => {
  ({ usePreloadChats, PRELOADS_AT_ONCE } = await import('./usePreloadChats'));
});

afterAll(() => {
  mock.restore();
});

const listed = (id: string): Chat => ({
  id,
  title: `Chat ${id}`,
  model_name: 'gpt-4',
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
  archived: false,
  agent_enabled: false,
});

const loaded = (id: string, content = `History of ${id}`): ChatWithMessages => ({
  ...listed(id),
  messages: [
    { id: `${id}-m`, chat_id: id, role: 'user', content, created_at: '2024-01-01T00:00:00Z' },
  ],
});

describe('usePreloadChats', () => {
  let pending: Map<string, (chat: ChatWithMessages) => void>;

  beforeEach(() => {
    pending = new Map();
    mockGetChat.mockReset();
    mockGetChat.mockImplementation(
      (id: string) =>
        new Promise<ChatWithMessages>((resolve) => {
          pending.set(id, resolve);
        })
    );
  });

  const settle = async (id: string, chat = loaded(id)) => {
    await act(async () => pending.get(id)?.(chat));
  };

  it('loads every listed chat into the cache, a few at a time', async () => {
    const chats = ['a', 'b', 'c', 'd', 'e'].map(listed);
    renderHook(() => usePreloadChats(chats));

    expect(mockGetChat).toHaveBeenCalledTimes(PRELOADS_AT_ONCE);
    for (const id of ['a', 'b', 'c', 'd', 'e']) {
      await waitFor(() => expect(pending.has(id)).toBe(true));
      await settle(id);
    }

    expect(mockGetChat).toHaveBeenCalledTimes(5);
    for (const id of ['a', 'b', 'c', 'd', 'e']) {
      expect(histories.recall(id)?.messages[0]?.content).toBe(`History of ${id}`);
    }
  });

  it('loads only the chats a new page adds', async () => {
    const { rerender } = renderHook(({ chats }) => usePreloadChats(chats), {
      initialProps: { chats: [listed('a')] },
    });
    await settle('a');

    rerender({ chats: [listed('a'), listed('b')] });

    await waitFor(() => expect(mockGetChat).toHaveBeenCalledTimes(2));
    expect(mockGetChat).toHaveBeenLastCalledWith('b');
  });

  it('leaves alone a chat the cache already holds', async () => {
    histories.remember(loaded('a', 'Already open'));

    renderHook(() => usePreloadChats([listed('a')]));

    expect(mockGetChat).not.toHaveBeenCalled();
    expect(histories.recall('a')?.messages[0]?.content).toBe('Already open');
  });

  it('keeps a history opened while its preload was still loading', async () => {
    renderHook(() => usePreloadChats([listed('a')]));
    histories.remember(loaded('a', 'Streamed while open'));

    await settle('a', loaded('a', 'Older copy'));

    expect(histories.recall('a')?.messages[0]?.content).toBe('Streamed while open');
  });

  it('drops a preload that lands after the cache was cleared by a logout', async () => {
    renderHook(() => usePreloadChats([listed('a')]));
    histories.clear();

    await settle('a');

    expect(histories.has('a')).toBe(false);
  });

  it('tries a chat again on the next listing after its preload failed', async () => {
    mockGetChat.mockRejectedValueOnce(new Error('Busy'));
    const { rerender } = renderHook(({ chats }) => usePreloadChats(chats), {
      initialProps: { chats: [listed('a')] },
    });
    await waitFor(() => expect(mockGetChat).toHaveBeenCalledTimes(1));
    await act(async () => {});

    rerender({ chats: [listed('a')] });

    await waitFor(() => expect(mockGetChat).toHaveBeenCalledTimes(2));
  });

  it('starts nothing more once unmounted', async () => {
    const chats = ['a', 'b', 'c', 'd'].map(listed);
    const { unmount } = renderHook(() => usePreloadChats(chats));
    unmount();

    await settle('a');

    expect(mockGetChat).toHaveBeenCalledTimes(PRELOADS_AT_ONCE);
  });
});
