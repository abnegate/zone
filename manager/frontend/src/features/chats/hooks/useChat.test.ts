import { renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import type { ChatWithMessages, Message } from '../types';
import { createElement } from 'react';
import type { ReactNode } from 'react';

const mockGetChat = mock();
const mockSendMessage = mock();
const mockDeleteMessage = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: {
    getChat: mockGetChat,
    sendMessage: mockSendMessage,
    deleteMessage: mockDeleteMessage,
  },
}));

let useChat: typeof import('./useChat').useChat;

beforeAll(async () => {
  ({ useChat } = await import('./useChat'));
});

afterAll(() => {
  mock.restore();
});

const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false, gcTime: 0 },
    },
  });
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
};

describe('useChat', () => {
  const mockMessages: Message[] = [
    {
      id: 'm1',
      chat_id: '1',
      role: 'user',
      content: 'Hello',
      created_at: '2024-01-01T00:00:00Z',
    },
    {
      id: 'm2',
      chat_id: '1',
      role: 'assistant',
      content: 'Hi there!',
      created_at: '2024-01-01T00:01:00Z',
    },
  ];

  const mockChat: ChatWithMessages = {
    id: '1',
    title: 'Test Chat',
    model_name: 'gpt-4',
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:01:00Z',
    archived: false,
    messages: mockMessages,
  };

  beforeEach(() => {
    mockGetChat.mockReset();
    mockSendMessage.mockReset();
    mockDeleteMessage.mockReset();
  });

  it('should fetch chat on mount', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.chat).toEqual(mockChat);
    expect(result.current.error).toBeNull();
    expect(mockGetChat).toHaveBeenCalledWith('1');
  });

  it('should handle errors when fetching chat', async () => {
    const error = new Error('Failed to fetch chat');
    mockGetChat.mockRejectedValue(error);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.chat).toBeNull();
    expect(result.current.error).toBe('Failed to fetch chat');
  });

  it('should send a message', async () => {
    const newMessage: Message = {
      id: 'm3',
      chat_id: '1',
      role: 'user',
      content: 'New message',
      created_at: '2024-01-01T00:02:00Z',
    };
    mockGetChat.mockResolvedValue(mockChat);
    mockSendMessage.mockResolvedValue(newMessage);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const message = await result.current.sendMessage({ content: 'New message' });

    expect(message).toEqual(newMessage);
    expect(mockSendMessage).toHaveBeenCalledWith('1', { content: 'New message' });
    await waitFor(() => {
      expect(result.current.chat?.messages).toHaveLength(3);
    });
    expect(result.current.chat?.messages).toContainEqual(newMessage);
  });

  it('should delete a message', async () => {
    mockGetChat.mockResolvedValue(mockChat);
    mockDeleteMessage.mockResolvedValue(undefined);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.deleteMessage('m1');

    expect(mockDeleteMessage).toHaveBeenCalledWith('1', 'm1');
    await waitFor(() => {
      expect(result.current.chat?.messages).toHaveLength(1);
    });
    expect(result.current.chat?.messages).not.toContainEqual(mockMessages[0]);
  });

  it('should refresh chat', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const updatedChat: ChatWithMessages = {
      ...mockChat,
      messages: [
        ...mockMessages,
        {
          id: 'm3',
          chat_id: '1',
          role: 'user',
          content: 'Updated message',
          created_at: '2024-01-01T00:02:00Z',
        },
      ],
    };
    mockGetChat.mockResolvedValue(updatedChat);

    await result.current.refresh();

    await waitFor(() => {
      expect(result.current.chat).toEqual(updatedChat);
    });
  });

  it('should handle sending message with error', async () => {
    const error = new Error('Failed to send message');
    mockGetChat.mockResolvedValue(mockChat);
    mockSendMessage.mockRejectedValue(error);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await expect(result.current.sendMessage({ content: 'Test' })).rejects.toThrow(
      'Failed to send message'
    );
    expect(result.current.chat?.messages).toHaveLength(2); // No new message added
  });

  it('should not fetch when chatId is null', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat(null), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.chat).toBeNull();
    expect(mockGetChat).not.toHaveBeenCalled();
  });
});
