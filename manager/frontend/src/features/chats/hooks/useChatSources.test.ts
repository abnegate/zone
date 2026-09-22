import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ChatSource } from '../types';

const mockGetChatSources = mock();
const mockSetChatSources = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: {
    getChatSources: mockGetChatSources,
    setChatSources: mockSetChatSources,
  },
}));

let useChatSources: typeof import('./useChatSources').useChatSources;

beforeAll(async () => {
  ({ useChatSources } = await import('./useChatSources'));
});

afterAll(() => {
  mock.restore();
});

const repository: ChatSource = {
  id: 'source-repo',
  name: 'abnegate/zone-tests',
  source_type: 'github',
};
const notes: ChatSource = { id: 'source-notes', name: 'Team notes', source_type: 'text' };

describe('useChatSources', () => {
  beforeEach(() => {
    mockGetChatSources.mockReset();
    mockSetChatSources.mockReset();
  });

  it('loads the attachment of the selected chat and reloads when the chat changes', async () => {
    mockGetChatSources.mockImplementation(async (chatId: string) =>
      chatId === 'chat-1' ? [repository] : [notes]
    );

    const { result, rerender } = renderHook(({ chatId }) => useChatSources(chatId), {
      initialProps: { chatId: 'chat-1' as string | null },
    });

    await waitFor(() => {
      expect(result.current.sources).toEqual([repository]);
    });
    expect(result.current.loading).toBe(false);

    rerender({ chatId: 'chat-2' });
    await waitFor(() => {
      expect(result.current.sources).toEqual([notes]);
    });
    expect(mockGetChatSources).toHaveBeenCalledWith('chat-2');

    rerender({ chatId: null });
    await waitFor(() => {
      expect(result.current.sources).toEqual([]);
    });
    expect(mockGetChatSources).toHaveBeenCalledTimes(2);
  });

  it('persists a new attachment through the API and keeps what the server answered', async () => {
    mockGetChatSources.mockResolvedValue([]);
    mockSetChatSources.mockResolvedValue([repository, notes]);

    const { result } = renderHook(() => useChatSources('chat-1'));
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await result.current.setAttached(['source-repo', 'source-notes']);
    });

    expect(mockSetChatSources).toHaveBeenCalledWith('chat-1', ['source-repo', 'source-notes']);
    expect(result.current.sources).toEqual([repository, notes]);
    expect(result.current.error).toBeNull();
  });

  it('surfaces a refused attachment and keeps the previous one', async () => {
    mockGetChatSources.mockResolvedValue([repository]);
    mockSetChatSources.mockRejectedValue(
      new Error("Failed to attach sources: Not sources of this chat's workspace: source-x")
    );

    const { result } = renderHook(() => useChatSources('chat-1'));
    await waitFor(() => {
      expect(result.current.sources).toEqual([repository]);
    });

    await act(async () => {
      await result.current.setAttached(['source-repo', 'source-x']);
    });

    expect(result.current.error).toContain('Not sources of this chat');
    expect(result.current.sources).toEqual([repository]);
  });

  it('does not call the API without a chat', async () => {
    const { result } = renderHook(() => useChatSources(null));
    await act(async () => {
      await result.current.setAttached(['source-repo']);
    });
    expect(mockGetChatSources).not.toHaveBeenCalled();
    expect(mockSetChatSources).not.toHaveBeenCalled();
  });
});
