import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import { ContextUsageSchema } from '../schemas';
import type {
  ActionReceipt,
  ChatCharacter,
  ChatWithMessages,
  Citation,
  ContextUsage,
  Message,
  MessageMetadata,
  MessageRole,
  SendMessageRequest,
  ToolCallRecord,
  UpdateChatRequest,
} from '../types';
import { mergeCitations } from '../utils/citations';

// The server saves the user message and streams the assistant reply over
// /ws/chats/:id. Posting to /api/chats/:id/messages only stores the user's
// message, so sending over the socket is what produces a reply.
type ServerMessage =
  | { type: 'context'; chat_id: string; message_id: string | null; usage: ContextUsage }
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
  | { type: 'reasoning'; content: string }
  | {
      type: 'tool_call';
      message_id: string;
      tool_call_id: string;
      name: string;
      arguments: string;
    }
  | {
      type: 'tool_approval_required';
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
      citations?: Citation[];
    }
  | {
      type: 'action_receipt';
      message_id: string;
      receipt: ActionReceipt;
    }
  | {
      type: 'image';
      message_id: string;
      attachment: NonNullable<MessageMetadata['attachments']>[number];
    }
  | {
      type: 'video';
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
  onTitleUpdated?: (id: string, title: string) => void,
  draft?: SendMessageRequest
) {
  const [chat, setChat] = useState<ChatWithMessages | null>(null);
  const [loading, setLoading] = useState(Boolean(chatId));
  const [error, setError] = useState<string | null>(null);
  const [streaming, setStreaming] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [context, setContext] = useState<ContextUsage | null>(null);
  const [contextError, setContextError] = useState<string | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [contextRefresh, setContextRefresh] = useState(0);
  const contextEpoch = useRef(0);
  const contextModel = useRef<string | null>(null);
  contextModel.current = chat?.id === chatId ? chat.model_name : null;
  const previewSequence = useRef(0);
  const socketRef = useRef<WebSocket | null>(null);
  const requestIdRef = useRef(0);
  const pendingUserIdRef = useRef<string | null>(null);
  const supersededPendingIdsRef = useRef<Set<string>>(new Set());
  const activeGenerationRef = useRef(false);
  const titleCallback = useRef(onTitleUpdated);
  titleCallback.current = onTitleUpdated;
  const titles = useRef(new Map<string, { title: string; revision: number }>());
  // A committed automatic title can arrive over the socket after a newer
  // manual PUT finishes. Keep that decision across conversation switches.
  const renamed = useRef(new Set<string>());
  const revision = useRef(0);

  const applyTitle = useCallback((id: string, title: string): void => {
    titles.current.set(id, { title, revision: ++revision.current });
    setChat((previous) => (previous?.id === id ? { ...previous, title } : previous));
  }, []);

  const updateTitle = useCallback(
    (id: string, title: string): void => {
      renamed.current.add(id);
      applyTitle(id, title);
    },
    [applyTitle]
  );

  const fetchChat = useCallback(
    async (opts?: { silent?: boolean }) => {
      const requestId = ++requestIdRef.current;
      const currentRevision = revision.current;
      const epoch = contextEpoch.current;
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
        const updated =
          title && title.revision > currentRevision ? { ...data, title: title.title } : data;
        setChat(updated);
        if (epoch === contextEpoch.current) {
          setContext(
            data.context ? (ContextUsageSchema.safeParse(data.context).data ?? null) : null
          );
        }
        // Only the selected chat has a socket, so returning to a conversation
        // must also reconcile any title generated while it was unselected.
        titleCallback.current?.(updated.id, updated.title);
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
    contextEpoch.current += 1;
    previewSequence.current += 1;
    setContext(null);
    setContextError(null);
    fetchChat();
  }, [fetchChat]);

  const draftKey = draft === undefined ? null : JSON.stringify(draft);
  const settingsKey = chat
    ? JSON.stringify([
        chat.model_name,
        chat.agent_enabled,
        chat.character,
        chat.auto_approve,
        chat.messages.length,
      ])
    : null;
  const identity = `${chatId}:${settingsKey}:${draftKey}:${contextRefresh}`;
  const previewIdentity = useRef(identity);
  previewIdentity.current = identity;
  useEffect(() => {
    const sequence = ++previewSequence.current;
    if (!chatId || chat?.id !== chatId || draftKey === null || settingsKey === null || streaming) {
      setPreviewing(false);
      return;
    }
    const controller = new AbortController();
    const epoch = contextEpoch.current;
    setPreviewing(true);
    setContextError(null);
    const timer = setTimeout(async () => {
      try {
        const usage = await chatsApi.previewContext(
          chatId,
          JSON.parse(draftKey),
          controller.signal
        );
        if (
          controller.signal.aborted ||
          sequence !== previewSequence.current ||
          epoch !== contextEpoch.current ||
          identity !== previewIdentity.current
        )
          return;
        const parsed = ContextUsageSchema.safeParse(usage);
        if (!parsed.success || parsed.data.model !== chat.model_name)
          throw new Error('Invalid context preview');
        setContext(parsed.data);
      } catch {
        if (
          controller.signal.aborted ||
          sequence !== previewSequence.current ||
          epoch !== contextEpoch.current ||
          identity !== previewIdentity.current
        )
          return;
        setContext(null);
        setContextError('Context preview unavailable. Your messages are unchanged.');
      } finally {
        if (!controller.signal.aborted && sequence === previewSequence.current)
          setPreviewing(false);
      }
    }, 300);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [chatId, chat?.id, chat?.model_name, draftKey, settingsKey, streaming, identity]);

  const upsertMessage = useCallback(
    (id: string, role: MessageRole, content: string, metadata?: MessageMetadata | null) => {
      setChat((prev) => {
        if (!prev) return prev;
        // message_saved can win the race (and Strict Mode can replay this
        // updater). Never revive a pending row that was already replaced.
        if (supersededPendingIdsRef.current.has(id)) {
          return prev;
        }
        const last = prev.messages[prev.messages.length - 1];
        if (last?.id === id) {
          return {
            ...prev,
            messages: [
              ...prev.messages.slice(0, -1),
              {
                ...last,
                content,
                // Merged, not replaced: a streaming turn has several
                // writers for one message, and an arriving image must not
                // drop the tool trace that patchToolCall put there.
                metadata:
                  metadata !== undefined ? { ...last.metadata, ...metadata } : last.metadata,
              },
            ],
          };
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

  const appendCitations = useCallback((messageId: string, incoming: Citation[]) => {
    if (incoming.length === 0) return;
    setChat((prev) => {
      if (!prev) return prev;
      return {
        ...prev,
        messages: prev.messages.map((message) =>
          message.id === messageId
            ? {
                ...message,
                metadata: {
                  ...message.metadata,
                  citations: mergeCitations(message.metadata?.citations, incoming),
                },
              }
            : message
        ),
      };
    });
  }, []);

  const appendReceipt = useCallback((messageId: string, receipt: ActionReceipt) => {
    setChat((prev) => {
      if (!prev) return prev;
      return {
        ...prev,
        messages: prev.messages.map((message) => {
          if (message.id !== messageId) return message;
          const existing = message.metadata?.action_receipts ?? [];
          const receipts = existing.some((item) => item.id === receipt.id)
            ? existing.map((item) => (item.id === receipt.id ? receipt : item))
            : [...existing, receipt];
          return { ...message, metadata: { ...message.metadata, action_receipts: receipts } };
        }),
      };
    });
  }, []);

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
    let generationSeen = false;
    const completed = new Set<string>();
    let assistantContent = '';
    let assistantMetadata: MessageMetadata | undefined;
    let chunkFrame = 0;
    const flushChunks = () => {
      chunkFrame = 0;
      if (assistantId) {
        upsertMessage(assistantId, 'assistant', assistantContent, assistantMetadata);
      }
    };
    const cancelChunkFrame = () => {
      if (!chunkFrame) return;
      if (typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(chunkFrame);
      } else {
        clearTimeout(chunkFrame);
      }
      chunkFrame = 0;
    };
    const flushChunksNow = () => {
      if (!chunkFrame) return;
      cancelChunkFrame();
      flushChunks();
    };
    const scheduleChunks = () => {
      if (chunkFrame) return;
      chunkFrame =
        typeof requestAnimationFrame === 'function'
          ? requestAnimationFrame(flushChunks)
          : window.setTimeout(flushChunks, 16);
    };

    const discardEmptyAssistant = (identifier: string | null): void => {
      if (!identifier) return;
      flushChunksNow();
      setChat((previous) =>
        previous
          ? {
              ...previous,
              messages: previous.messages.filter(
                (message) =>
                  message.id !== identifier ||
                  message.role !== 'assistant' ||
                  message.content.trim().length > 0 ||
                  Boolean(
                    message.metadata?.attachments?.length ||
                      message.metadata?.tool_calls?.length ||
                      message.metadata?.citations?.length ||
                      message.metadata?.action_receipts?.length
                  )
              ),
            }
          : previous
      );
    };

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

      if (payload.type !== 'chunk') {
        flushChunksNow();
      }

      switch (payload.type) {
        case 'context': {
          if (payload.chat_id !== chatId) break;
          if (
            payload.message_id === null
              ? generationSeen || activeGenerationRef.current
              : payload.message_id !== assistantId
          )
            break;
          const parsed = ContextUsageSchema.safeParse(payload.usage);
          if (!parsed.success || parsed.data.model !== contextModel.current) break;
          contextEpoch.current += 1;
          setContext(parsed.data);
          setContextError(null);
          setPreviewing(false);
          break;
        }
        case 'title_updated':
          if (payload.chat_id === chatId && !renamed.current.has(payload.chat_id)) {
            applyTitle(payload.chat_id, payload.title);
            titleCallback.current?.(payload.chat_id, payload.title);
          }
          break;
        case 'message_saved':
          if (payload.role === 'user') {
            applySavedUserMessage(payload.message_id, payload.content, payload.metadata);
          } else {
            upsertMessage(payload.message_id, payload.role, payload.content, payload.metadata);
          }
          break;
        case 'status':
          setStatus(payload.message);
          break;
        case 'message_start':
          if (completed.has(payload.message_id) || assistantId === payload.message_id) break;
          activeGenerationRef.current = true;
          setStreaming(true);
          generationSeen = true;
          contextEpoch.current += 1;
          setStatus(null);
          assistantId = payload.message_id;
          assistantContent = '';
          assistantMetadata = undefined;
          upsertMessage(payload.message_id, payload.role, '');
          break;
        case 'chunk':
          if (assistantId) {
            assistantContent += payload.content;
            scheduleChunks();
          }
          break;
        case 'reasoning':
          if (assistantId) {
            assistantMetadata = {
              ...assistantMetadata,
              reasoning: `${assistantMetadata?.reasoning ?? ''}${payload.content}`,
            };
            upsertMessage(assistantId, 'assistant', assistantContent, assistantMetadata);
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
        case 'tool_approval_required':
          patchToolCall(payload.message_id, payload.tool_call_id, {
            name: payload.name,
            arguments: payload.arguments,
            detail: 'Waiting for approval…',
            pending: true,
            approval: 'pending',
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
          if (payload.citations?.length) {
            appendCitations(payload.message_id, payload.citations);
          }
          break;
        case 'action_receipt':
          appendReceipt(payload.message_id, payload.receipt);
          break;
        case 'image':
        case 'video':
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
          if (
            completed.has(payload.message_id) ||
            (assistantId !== null && payload.message_id !== assistantId)
          )
            break;
          completed.add(payload.message_id);
          contextEpoch.current += 1;
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
          if (
            payload.message_id === null
              ? assistantId !== null
              : completed.has(payload.message_id) ||
                (assistantId !== null && payload.message_id !== assistantId)
          )
            break;
          if (payload.message_id !== null) completed.add(payload.message_id);
          discardEmptyAssistant(assistantId ?? payload.message_id);
          assistantId = null;
          contextEpoch.current += 1;
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
          discardEmptyAssistant(assistantId);
          if (assistantId !== null) completed.add(assistantId);
          assistantId = null;
          contextEpoch.current += 1;
          setStatus(null);
          setError(payload.message);
          activeGenerationRef.current = false;
          setStreaming(false);
          break;
        default:
          break;
      }
    };

    const invalidateContext = (): void => {
      discardEmptyAssistant(assistantId);
      if (assistantId !== null) completed.add(assistantId);
      const interrupted = activeGenerationRef.current;
      setContext((previous) =>
        previous
          ? {
              ...previous,
              status: 'unavailable',
              incomplete: true,
              reason: interrupted
                ? 'Connection interrupted during generation. Usage is the last observation until a fresh preview is available.'
                : 'Connection closed. Usage is the last observation until a fresh preview is available.',
            }
          : null
      );
      setContextRefresh((value) => value + 1);
    };

    socket.onerror = () => {
      invalidateContext();
      assistantId = null;
      contextEpoch.current += 1;
      setError('Chat connection failed');
      setStatus(null);
      activeGenerationRef.current = false;
      setStreaming(false);
    };

    socket.onclose = () => {
      invalidateContext();
      assistantId = null;
      contextEpoch.current += 1;
      setStatus(null);
      activeGenerationRef.current = false;
      setStreaming(false);
    };

    return () => {
      cancelChunkFrame();
      socket.onopen = null;
      socket.onmessage = null;
      socket.onerror = null;
      socket.onclose = null;
      socket.close();
      socketRef.current = null;
    };
  }, [
    chatId,
    upsertMessage,
    applySavedUserMessage,
    patchToolCall,
    appendCitations,
    appendReceipt,
    applyTitle,
  ]);

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
    contextEpoch.current += 1;
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

  const approveTool = (toolCallId: string, approved: boolean): void => {
    const socket = socketRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      return;
    }
    const assistant = chat?.messages.find((message) =>
      message.metadata?.tool_calls?.some((call) => call.id === toolCallId)
    );
    if (assistant) {
      patchToolCall(assistant.id, toolCallId, {
        approval: approved ? 'approved' : 'denied',
        detail: approved ? 'Approved. Running…' : 'Denied',
        pending: approved,
      });
    }
    socket.send(
      JSON.stringify({
        type: 'approve_tool',
        tool_call_id: toolCallId,
        approved,
      })
    );
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
            ...updated,
            messages:
              'messages' in updated && Array.isArray(updated.messages)
                ? updated.messages
                : prev.messages,
          }
        : prev
    );
  };

  const setAgentEnabled = (enabled: boolean): Promise<void> =>
    updateAgentSettings({ agent_enabled: enabled });

  const setAutoApprove = (enabled: boolean): Promise<void> =>
    updateAgentSettings({ auto_approve: enabled });

  const setCharacter = (character: ChatCharacter): Promise<void> =>
    updateAgentSettings({ character });

  const clearCharacter = (): Promise<void> => updateAgentSettings({ clear_character: true });

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
    context: context?.model === chat?.model_name ? context : null,
    contextError,
    previewing,
    loading,
    error,
    streaming,
    status,
    sendMessage,
    cancelGeneration,
    approveTool,
    setAgentEnabled,
    setAutoApprove,
    setCharacter,
    clearCharacter,
    deleteMessage,
    refresh,
    updateTitle,
  };
}
