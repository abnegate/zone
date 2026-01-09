import { renderHook, waitFor } from '@testing-library/react';

import { useChat } from './useChat';
import { chatsApi } from '../../../api/chats';
import type { ChatWithMessages, Message } from '../types';

jest.mock('../../../api/chats');

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
    jest.clearAllMocks();
  });

  it('should fetch chat on mount', async () => {
    (chatsApi.getChat as jest.Mock).mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'));

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.chat).toEqual(mockChat);
    expect(result.current.error).toBeNull();
    expect(chatsApi.getChat).toHaveBeenCalledWith('1');
  });

  it('should handle errors when fetching chat', async () => {
    const error = new Error('Failed to fetch chat');
    (chatsApi.getChat as jest.Mock).mockRejectedValue(error);

    const { result } = renderHook(() => useChat('1'));

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
    (chatsApi.getChat as jest.Mock).mockResolvedValue(mockChat);
    (chatsApi.sendMessage as jest.Mock).mockResolvedValue(newMessage);

    const { result } = renderHook(() => useChat('1'));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const message = await result.current.sendMessage({ content: 'New message' });

    expect(message).toEqual(newMessage);
    expect(chatsApi.sendMessage).toHaveBeenCalledWith('1', { content: 'New message' });
    await waitFor(() => {
      expect(result.current.chat?.messages).toHaveLength(3);
    });
    expect(result.current.chat?.messages).toContainEqual(newMessage);
  });

  it('should delete a message', async () => {
    (chatsApi.getChat as jest.Mock).mockResolvedValue(mockChat);
    (chatsApi.deleteMessage as jest.Mock).mockResolvedValue(undefined);

    const { result } = renderHook(() => useChat('1'));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.deleteMessage('m1');

    expect(chatsApi.deleteMessage).toHaveBeenCalledWith('1', 'm1');
    await waitFor(() => {
      expect(result.current.chat?.messages).toHaveLength(1);
    });
    expect(result.current.chat?.messages).not.toContainEqual(mockMessages[0]);
  });

  it('should refresh chat', async () => {
    (chatsApi.getChat as jest.Mock).mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'));

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
    (chatsApi.getChat as jest.Mock).mockResolvedValue(updatedChat);

    await result.current.refresh();

    await waitFor(() => {
      expect(result.current.chat).toEqual(updatedChat);
    });
  });

  it('should handle sending message with error', async () => {
    const error = new Error('Failed to send message');
    (chatsApi.getChat as jest.Mock).mockResolvedValue(mockChat);
    (chatsApi.sendMessage as jest.Mock).mockRejectedValue(error);

    const { result } = renderHook(() => useChat('1'));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await expect(result.current.sendMessage({ content: 'Test' })).rejects.toThrow(
      'Failed to send message'
    );
    expect(result.current.chat?.messages).toHaveLength(2); // No new message added
  });

  it('should not fetch when chatId is null', async () => {
    (chatsApi.getChat as jest.Mock).mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat(null));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.chat).toBeNull();
    expect(chatsApi.getChat).not.toHaveBeenCalled();
  });
});
