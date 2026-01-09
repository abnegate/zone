import { useState, useEffect, useCallback } from 'react';
import { chatsApi } from '../../../api/chats';
import type { ChatWithMessages, Message, SendMessageRequest } from '../types';

export function useChat(chatId: string | null) {
  const [chat, setChat] = useState<ChatWithMessages | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchChat = useCallback(async () => {
    if (!chatId) {
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);
    try {
      const data = await chatsApi.getChat(chatId);
      setChat(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to fetch chat');
    } finally {
      setLoading(false);
    }
  }, [chatId]);

  useEffect(() => {
    fetchChat();
  }, [fetchChat]);

  const sendMessage = async (request: SendMessageRequest): Promise<Message> => {
    if (!chatId) {
      throw new Error('No chat selected');
    }
    const message = await chatsApi.sendMessage(chatId, request);
    setChat((prev) => (prev ? { ...prev, messages: [...prev.messages, message] } : null));
    return message;
  };

  const deleteMessage = async (messageId: string): Promise<void> => {
    if (!chatId) {
      throw new Error('No chat selected');
    }
    await chatsApi.deleteMessage(chatId, messageId);
    setChat((prev) =>
      prev ? { ...prev, messages: prev.messages.filter((m) => m.id !== messageId) } : null
    );
  };

  const refresh = async (): Promise<void> => {
    await fetchChat();
  };

  return {
    chat,
    loading,
    error,
    sendMessage,
    deleteMessage,
    refresh,
  };
}
