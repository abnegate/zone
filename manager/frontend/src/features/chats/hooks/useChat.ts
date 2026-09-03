import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type {
  ChatWithMessages,
  Message,
  MessageMetadata,
  MessageRole,
  SendMessageRequest,
} from '../types';

// The server saves the user message and streams the assistant reply over
// /ws/chats/:id. Posting to /api/chats/:id/messages only stores the user's
// message, so sending over the socket is what produces a reply.
type ServerMessage =
  | { type: 'init'; chat_id: string; status: string }
  | {
      type: 'message_saved';
      message_id: string;
      role: MessageRole;
      content: string;
      metadata?: MessageMetadata | null;
    }
  | { type: 'message_start'; message_id: string; role: MessageRole }
  | { type: 'chunk'; content: string; index: number }
  | {
      type: 'image';
      message_id: string;
      attachment: NonNullable<MessageMetadata['attachments']>[number];
    }
  | {
      type: 'message_end';
      message_id: string;
      content: string;
      metadata?: MessageMetadata | null;
    }
  | { type: 'cancelled'; message_id: string | null }
  | { type: 'error'; message: string }
  | { type: 'status'; message: string };

export function useChat(chatId: string | null) {
  const [chat, setChat] = useState<ChatWithMessages | null>(null);
  const [loading, setLoading] = useState(Boolean(chatId));
  const [error, setError] = useState<string | null>(null);
  const [streaming, setStreaming] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const socketRef = useRef<WebSocket | null>(null);
  const requestIdRef = useRef(0);
  const pendingUserIdRef = useRef<string | null>(null);

  const fetchChat = useCallback(
    async (opts?: { silent?: boolean }) => {
      const requestId = ++requestIdRef.current;
      if (!chatId) {
        setChat(null);
        setLoading(false);
        setError(null);
        return;
      }

      if (!opts?.silent) {
        setLoading(true);
      }
      setError(null);
      try {
        const data = await chatsApi.getChat(chatId);
        if (requestId !== requestIdRef.current) return;
        setChat(data);
      } catch (err) {
        if (requestId !== requestIdRef.current) return;
        setError(err instanceof Error ? err.message : 'Failed to fetch chat');
      } finally {
        if (requestId === requestIdRef.current) {
          setLoading(false);
        }
      }
    },
    [chatId]
  );

  useEffect(() => {
    // Drop the previous conversation as soon as the selection changes so the
    // UI never keeps rendering chat A under chat B's selection.
    setChat(null);
    fetchChat();
  }, [fetchChat]);

  const upsertMessage = useCallback(
    (id: string, role: MessageRole, content: string, metadata?: MessageMetadata | null) => {
      setChat((prev) => {
        if (!prev) return prev;
        const existing = prev.messages.find((m) => m.id === id);
        if (existing) {
          return {
            ...prev,
            messages: prev.messages.map((m) =>
              m.id === id
                ? {
                    ...m,
                    content,
                    metadata: metadata !== undefined ? (metadata ?? undefined) : m.metadata,
                  }
                : m
            ),
          };
        }
        const message: Message = {
          id,
          chat_id: prev.id,
          role,
          content,
          created_at: new Date().toISOString(),
          metadata: metadata ?? undefined,
        };
        return { ...prev, messages: [...prev.messages, message] };
      });
    },
    []
  );

  const applySavedUserMessage = useCallback(
    (id: string, content: string, metadata?: MessageMetadata | null) => {
      setChat((prev) => {
        if (!prev) return prev;
        const pendingId = pendingUserIdRef.current;
        pendingUserIdRef.current = null;
        const replaceId =
          pendingId && prev.messages.some((m) => m.id === pendingId) ? pendingId : id;
        if (prev.messages.some((m) => m.id === replaceId)) {
          return {
            ...prev,
            messages: prev.messages.map((m) =>
              m.id === replaceId ? { ...m, id, content, metadata: metadata ?? m.metadata } : m
            ),
          };
        }
        const message: Message = {
          id,
          chat_id: prev.id,
          role: 'user',
          content,
          created_at: new Date().toISOString(),
          metadata: metadata ?? undefined,
        };
        return { ...prev, messages: [...prev.messages, message] };
      });
    },
    []
  );

  useEffect(() => {
    if (!chatId) {
      return;
    }

    setStatus(null);
    const socket = chatsApi.createChatWebSocket(chatId);
    socketRef.current = socket;
    let assistantId: string | null = null;
    let assistantContent = '';
    let assistantMetadata: MessageMetadata | undefined;

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
          applySavedUserMessage(payload.message_id, payload.content, payload.metadata);
          break;
        case 'status':
          setStatus(payload.message);
          break;
        case 'message_start':
          setStatus(null);
          assistantId = payload.message_id;
          assistantContent = '';
          assistantMetadata = undefined;
          upsertMessage(payload.message_id, payload.role, '');
          break;
        case 'chunk':
          if (assistantId) {
            assistantContent += payload.content;
            upsertMessage(assistantId, 'assistant', assistantContent);
          }
          break;
        case 'image':
          if (assistantId === payload.message_id) {
            const attachments = assistantMetadata?.attachments ?? [];
            assistantMetadata = {
              attachments: [
                ...attachments.filter((attachment) => attachment.url !== payload.attachment.url),
                payload.attachment,
              ],
            };
            upsertMessage(assistantId, 'assistant', assistantContent, assistantMetadata);
          }
          break;
        case 'message_end':
          setStatus(null);
          upsertMessage(
            payload.message_id,
            'assistant',
            payload.content,
            payload.metadata ?? assistantMetadata
          );
          assistantId = null;
          assistantContent = '';
          assistantMetadata = undefined;
          setStreaming(false);
          break;
        case 'cancelled':
          setStatus(null);
          setStreaming(false);
          break;
        case 'error':
          setStatus(null);
          setError(payload.message);
          setStreaming(false);
          break;
        default:
          break;
      }
    };

    socket.onerror = () => {
      setError('Chat connection failed');
      setStatus(null);
      setStreaming(false);
    };

    socket.onclose = () => {
      setStatus(null);
      setStreaming(false);
    };

    return () => {
      socket.close();
      socketRef.current = null;
    };
  }, [chatId, upsertMessage, applySavedUserMessage]);

  const waitForOpen = (socket: WebSocket, timeoutMs = 5000): Promise<void> => {
    if (socket.readyState === 1) {
      return Promise.resolve();
    }
    if (socket.readyState === 2 || socket.readyState === 3) {
      return Promise.reject(new Error('Chat connection is not open'));
    }
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error('Chat connection is not open'));
      }, timeoutMs);
      const finish = (ok: boolean) => {
        clearTimeout(timer);
        if (ok) resolve();
        else reject(new Error('Chat connection is not open'));
      };
      socket.addEventListener('open', () => finish(true), { once: true });
      socket.addEventListener('error', () => finish(false), { once: true });
      socket.addEventListener('close', () => finish(false), { once: true });
    });
  };

  const sendMessage = async (request: SendMessageRequest): Promise<void> => {
    if (!chatId) {
      throw new Error('No chat selected');
    }
    const socket = socketRef.current;
    if (!socket) {
      throw new Error('Chat connection is not open');
    }
    await waitForOpen(socket);
    setError(null);
    const pendingId = `pending-${crypto.randomUUID()}`;
    pendingUserIdRef.current = pendingId;
    upsertMessage(pendingId, 'user', request.content, request.metadata);
    setStreaming(true);
    try {
      socket.send(
        JSON.stringify({
          type: 'send',
          content: request.content,
          metadata: request.metadata,
        })
      );
    } catch (err) {
      pendingUserIdRef.current = null;
      setChat((prev) =>
        prev ? { ...prev, messages: prev.messages.filter((m) => m.id !== pendingId) } : prev
      );
      setStreaming(false);
      throw err;
    }
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
    await fetchChat({ silent: true });
  };

  return {
    chat,
    loading,
    error,
    streaming,
    status,
    sendMessage,
    cancelGeneration,
    deleteMessage,
    refresh,
  };
}
