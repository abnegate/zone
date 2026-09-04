import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type {
  ChatWithMessages,
  Message,
  MessageMetadata,
  MessageRole,
  SendMessageRequest,
  ToolCallRecord,
  UpdateChatRequest,
} from '../types';

// The server saves the user message and streams the assistant reply over
// /ws/chats/:id. Posting to /api/chats/:id/messages only stores the user's
// message, so sending over the socket is what produces a reply.
type ServerMessage =
  | { type: 'title_updated'; chat_id: string; title: string }
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
      type: 'tool_call';
      message_id: string;
      tool_call_id: string;
      name: string;
      arguments: string;
    }
  | {
      type: 'tool_result';
      message_id: string;
      tool_call_id: string;
      name: string;
      success: boolean;
      detail: string;
      duration_ms: number;
    }
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
      error?: string;
    }
  | { type: 'cancelled'; message_id: string | null }
  | { type: 'error'; message: string }
  | { type: 'status'; message: string };

export function useChat(
  chatId: string | null,
  onTitleUpdated?: (id: string, title: string) => void
) {
  const [chat, setChat] = useState<ChatWithMessages | null>(null);
  const [loading, setLoading] = useState(Boolean(chatId));
  const [error, setError] = useState<string | null>(null);
  const [streaming, setStreaming] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const socketRef = useRef<WebSocket | null>(null);
  const requestIdRef = useRef(0);
  const pendingUserIdRef = useRef<string | null>(null);
  const supersededPendingIdsRef = useRef<Set<string>>(new Set());
  const activeGenerationRef = useRef(false);
  const titleCallback = useRef(onTitleUpdated);
  titleCallback.current = onTitleUpdated;
  const titles = useRef(new Map<string, { title: string; revision: number }>());
  const revision = useRef(0);

  const updateTitle = useCallback((id: string, title: string): void => {
    titles.current.set(id, { title, revision: ++revision.current });
    setChat((previous) => (previous?.id === id ? { ...previous, title } : previous));
  }, []);

  const fetchChat = useCallback(
    async (opts?: { silent?: boolean }) => {
      const requestId = ++requestIdRef.current;
      const currentRevision = revision.current;
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
        const title = titles.current.get(data.id);
        setChat(title && title.revision > currentRevision ? { ...data, title: title.title } : data);
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
        // message_saved can win the race (and Strict Mode can replay this
        // updater). Never revive a pending row that was already replaced.
        if (supersededPendingIdsRef.current.has(id)) {
          return prev;
        }
        const existing = prev.messages.find((m) => m.id === id);
        if (existing) {
          return {
            ...prev,
            messages: prev.messages.map((m) =>
              m.id === id
                ? {
                    ...m,
                    content,
                    // Merged, not replaced: a streaming turn has several
                    // writers for one message, and an arriving image must not
                    // drop the tool trace that patchToolCall put there.
                    metadata: metadata !== undefined ? { ...m.metadata, ...metadata } : m.metadata,
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

  // Tool calls arrive in two frames: one when the agent starts a tool and one
  // when it finishes, so this merges a partial update into the message's trace
  // rather than replacing the record.
  const patchToolCall = useCallback(
    (messageId: string, toolCallId: string, patch: Partial<ToolCallRecord>) => {
      setChat((prev) => {
        if (!prev) return prev;
        return {
          ...prev,
          messages: prev.messages.map((message) => {
            if (message.id !== messageId) return message;
            const existing = message.metadata?.tool_calls ?? [];
            const toolCalls = existing.some((call) => call.id === toolCallId)
              ? existing.map((call) => (call.id === toolCallId ? { ...call, ...patch } : call))
              : [
                  ...existing,
                  {
                    id: toolCallId,
                    name: '',
                    arguments: '',
                    success: false,
                    detail: '',
                    duration_ms: 0,
                    ...patch,
                  },
                ];
            return { ...message, metadata: { ...message.metadata, tool_calls: toolCalls } };
          }),
        };
      });
    },
    []
  );

  const applySavedUserMessage = useCallback(
    (id: string, content: string, metadata?: MessageMetadata | null) => {
      // Read and clear refs outside setChat so the updater stays pure.
      // Strict Mode invokes updaters twice with the same prev; mutating the
      // ref inside the updater left the pending row and appended the saved one.
      const pendingId = pendingUserIdRef.current;
      pendingUserIdRef.current = null;
      if (pendingId) {
        supersededPendingIdsRef.current.add(pendingId);
      }

      setChat((prev) => {
        if (!prev) return prev;
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
    activeGenerationRef.current = false;
    pendingUserIdRef.current = null;
    supersededPendingIdsRef.current.clear();
    setStreaming(false);
    setStatus(null);
    if (!chatId) {
      return;
    }

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
      if (socket !== socketRef.current) return;
      let payload: ServerMessage;
      try {
        payload = JSON.parse(event.data);
      } catch {
        return;
      }

      switch (payload.type) {
        case 'title_updated':
          if (payload.chat_id === chatId) {
            updateTitle(payload.chat_id, payload.title);
            titleCallback.current?.(payload.chat_id, payload.title);
          }
          break;
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
        case 'tool_call':
          patchToolCall(payload.message_id, payload.tool_call_id, {
            name: payload.name,
            arguments: payload.arguments,
            detail: 'Running…',
            pending: true,
          });
          break;
        case 'tool_result':
          patchToolCall(payload.message_id, payload.tool_call_id, {
            name: payload.name,
            success: payload.success,
            detail: payload.detail,
            duration_ms: payload.duration_ms,
            pending: false,
          });
          break;
        case 'image':
          if (assistantId === payload.message_id) {
            const attachments = assistantMetadata?.attachments ?? [];
            assistantMetadata = {
              ...assistantMetadata,
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
          setError(payload.error ?? null);
          upsertMessage(
            payload.message_id,
            'assistant',
            payload.content,
            payload.metadata ?? assistantMetadata
          );
          assistantId = null;
          assistantContent = '';
          assistantMetadata = undefined;
          activeGenerationRef.current = false;
          setStreaming(false);
          break;
        case 'cancelled': {
          const pendingId = pendingUserIdRef.current;
          pendingUserIdRef.current = null;
          if (pendingId) {
            supersededPendingIdsRef.current.add(pendingId);
            setChat((prev) =>
              prev
                ? { ...prev, messages: prev.messages.filter((message) => message.id !== pendingId) }
                : prev
            );
          }
          setStatus(null);
          activeGenerationRef.current = false;
          setStreaming(false);
          break;
        }
        case 'error':
          setStatus(null);
          setError(payload.message);
          activeGenerationRef.current = false;
          setStreaming(false);
          break;
        default:
          break;
      }
    };

    socket.onerror = () => {
      setError('Chat connection failed');
      setStatus(null);
      activeGenerationRef.current = false;
      setStreaming(false);
    };

    socket.onclose = () => {
      setStatus(null);
      activeGenerationRef.current = false;
      setStreaming(false);
    };

    return () => {
      socket.onopen = null;
      socket.onmessage = null;
      socket.onerror = null;
      socket.onclose = null;
      socket.close();
      socketRef.current = null;
    };
  }, [chatId, upsertMessage, applySavedUserMessage, patchToolCall, updateTitle]);

  const waitForOpen = (socket: WebSocket, timeoutMs = 5000): Promise<void> => {
    // Numeric readyState values stay valid for test doubles that do not
    // implement the WebSocket.OPEN/CLOSING/CLOSED constants.
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
    if (socket !== socketRef.current) {
      throw new Error('Chat selection changed before the message was sent');
    }
    if (activeGenerationRef.current) {
      throw new Error('Wait for the current response to finish');
    }
    setError(null);
    const pendingId = `pending-${crypto.randomUUID()}`;
    pendingUserIdRef.current = pendingId;
    activeGenerationRef.current = true;
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
      activeGenerationRef.current = false;
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

  // Persisted on the chat rather than sent per message, so the next reply uses
  // the new mode whichever window or device it comes from.
  const updateAgentSettings = async (settings: UpdateChatRequest): Promise<void> => {
    if (!chatId) {
      throw new Error('No chat selected');
    }
    const updated = await chatsApi.updateChat(chatId, settings);
    setChat((prev) =>
      prev && prev.id === updated.id
        ? {
            ...prev,
            agent_enabled: updated.agent_enabled,
            agent_sandboxed: updated.agent_sandboxed,
          }
        : prev
    );
  };

  const setAgentEnabled = (enabled: boolean): Promise<void> =>
    updateAgentSettings({ agent_enabled: enabled });

  const setAgentSandboxed = (sandboxed: boolean): Promise<void> =>
    updateAgentSettings({ agent_sandboxed: sandboxed });

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
    setAgentEnabled,
    setAgentSandboxed,
    deleteMessage,
    refresh,
    updateTitle,
  };
}
