import { useState, useEffect, useCallback, useRef } from 'react';
import { chatsApi } from '../../../api/chats';
import type { ChatWithMessages, Message, MessageRole, SendMessageRequest } from '../types';

// The server saves the user message and streams the assistant reply over
// /ws/chats/:id. Posting to /api/chats/:id/messages only stores the user's
// message, so sending over the socket is what produces a reply.
type ServerMessage =
  | { type: 'init'; chat_id: string; status: string }
  | { type: 'message_saved'; message_id: string; role: MessageRole; content: string }
  | { type: 'message_start'; message_id: string; role: MessageRole }
  | { type: 'chunk'; content: string; index: number }
  | { type: 'message_end'; message_id: string; content: string }
  | { type: 'cancelled'; message_id: string | null }
  | { type: 'error'; message: string };

export function useChat(chatId: string | null) {
  const [chat, setChat] = useState<ChatWithMessages | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [streaming, setStreaming] = useState(false);
  const socketRef = useRef<WebSocket | null>(null);

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

  const upsertMessage = useCallback((id: string, role: MessageRole, content: string) => {
    setChat((prev) => {
      if (!prev) return prev;
      const existing = prev.messages.find((m) => m.id === id);
      if (existing) {
        return {
          ...prev,
          messages: prev.messages.map((m) => (m.id === id ? { ...m, content } : m)),
        };
      }
      const message: Message = {
        id,
        chat_id: prev.id,
        role,
        content,
        created_at: new Date().toISOString(),
      };
      return { ...prev, messages: [...prev.messages, message] };
    });
  }, []);

  useEffect(() => {
    if (!chatId) {
      return;
    }

    const socket = chatsApi.createChatWebSocket(chatId);
    socketRef.current = socket;
    let assistantId: string | null = null;
    let assistantContent = '';

    socket.onopen = () => {
      const token = chatsApi.chatAccessToken();
      if (token) {
        socket.send(JSON.stringify({ type: 'auth', token }));
      }
    };

    socket.onmessage = (event) => {
      let payload: ServerMessage;
      try {
        payload = JSON.parse(event.data);
      } catch {
        return;
      }

      switch (payload.type) {
        case 'message_saved':
          upsertMessage(payload.message_id, payload.role, payload.content);
          break;
        case 'message_start':
          assistantId = payload.message_id;
          assistantContent = '';
          upsertMessage(payload.message_id, payload.role, '');
          break;
        case 'chunk':
          if (assistantId) {
            assistantContent += payload.content;
            upsertMessage(assistantId, 'assistant', assistantContent);
          }
          break;
        case 'message_end':
          upsertMessage(payload.message_id, 'assistant', payload.content);
          assistantId = null;
          assistantContent = '';
          setStreaming(false);
          break;
        case 'cancelled':
          setStreaming(false);
          break;
        case 'error':
          setError(payload.message);
          setStreaming(false);
          break;
        default:
          break;
      }
    };

    socket.onerror = () => {
      setError('Chat connection failed');
      setStreaming(false);
    };

    socket.onclose = () => {
      setStreaming(false);
    };

    return () => {
      socket.close();
      socketRef.current = null;
    };
  }, [chatId, upsertMessage]);

  const sendMessage = async (request: SendMessageRequest): Promise<void> => {
    if (!chatId) {
      throw new Error('No chat selected');
    }
    const socket = socketRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      throw new Error('Chat connection is not open');
    }
    setError(null);
    setStreaming(true);
    socket.send(
      JSON.stringify({
        type: 'send',
        content: request.content,
        metadata: request.metadata,
      })
    );
  };

  const cancelGeneration = () => {
    const socket = socketRef.current;
    if (socket && socket.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ type: 'cancel' }));
    }
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
    streaming,
    sendMessage,
    cancelGeneration,
    deleteMessage,
    refresh,
  };
}
