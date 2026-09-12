import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import {
  ContextUsageSchema,
  JobExitedSchema,
  JobStartedSchema,
  QuestionsSchema,
  WaitingSchema,
  WaitSettledSchema,
} from '../schemas';
import {
  type ActionReceipt,
  AWAITING_ANSWER_DETAIL,
  type ChatCharacter,
  type ChatWithMessages,
  type Citation,
  type ContextUsage,
  type JobExited,
  type JobStarted,
  type Message,
  type MessageMetadata,
  type MessageRole,
  type Question,
  type ReasoningEffort,
  type SendMessageRequest,
  type ToolCallRecord,
  type UpdateChatRequest,
  type Waiting,
  type WaitSettled,
} from '../types';
import { mergeCitations } from '../utils/citations';

// How many frames to hold while the chat they belong to is still loading.
const MAX_HELD_FRAMES = 1000;

// The frames a background job and a wait arrive on. An exit carries no call
// id, only the job's own, so it is matched to the call that started the job
// rather than addressed to one.
const JOB_STARTED = 'job_started';
const JOB_EXITED = 'job_exited';
const WAIT_STARTED = 'wait_started';
const WAIT_SETTLED = 'wait_settled';

const EMPTY_TOOL_CALL: Omit<ToolCallRecord, 'id'> = {
  name: '',
  arguments: '',
  success: false,
  detail: '',
  duration_ms: 0,
};

// A frame names the call it patches by id, or by something only the message's
// existing calls can answer; either way an unknown call is created rather
// than dropped, so a frame that beat or outlived its call still renders.
type ToolCallTarget = string | ((calls: ToolCallRecord[]) => string);

function upsertToolCall(
  message: Message,
  target: ToolCallTarget,
  patch: Partial<ToolCallRecord>
): Message {
  const existing = message.metadata?.tool_calls ?? [];
  const toolCallId = typeof target === 'string' ? target : target(existing);
  const toolCalls = existing.some((call) => call.id === toolCallId)
    ? existing.map((call) => (call.id === toolCallId ? { ...call, ...patch } : call))
    : [...existing, { id: toolCallId, ...EMPTY_TOOL_CALL, ...patch }];
  return { ...message, metadata: { ...message.metadata, tool_calls: toolCalls } };
}

// The call that started this job, or the job id itself when no call on the
// message claims it, so an exit that arrived unpaired still has a row.
const startedBy =
  (jobId: string): ToolCallTarget =>
  (calls) =>
    calls.find((call) => call.job?.id === jobId)?.id ?? jobId;

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
  | { type: 'message_start'; message_id: string; role: MessageRole; resumed?: boolean }
  | { type: 'chunk'; content: string; index: number }
  | { type: 'reasoning'; content: string }
  | {
      type: 'tool_call';
      message_id: string;
      tool_call_id: string;
      name: string;
      arguments: string;
      reasoning?: string;
      reason?: string;
    }
  | {
      type: 'tool_approval_required';
      message_id: string;
      tool_call_id: string;
      name: string;
      arguments: string;
      reason?: string;
      preview?: string;
    }
  | { type: 'tool_approval_closed'; tool_call_id: string }
  | {
      type: 'question_required';
      message_id: string;
      tool_call_id: string;
      questions: Question[];
    }
  | { type: typeof JOB_STARTED; message_id: string; tool_call_id: string; job: JobStarted }
  | { type: typeof JOB_EXITED; message_id: string; job: JobExited }
  | { type: typeof WAIT_STARTED; message_id: string; tool_call_id: string; waiting: Waiting }
  | { type: typeof WAIT_SETTLED; message_id: string; settled: WaitSettled }
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
      type: 'audio';
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
  // The chat the socket handlers have to write into. Until the fetch lands
  // there is nowhere to put a frame, so the socket effect holds them.
  const loadedChat = useRef<string | null>(null);
  loadedChat.current = chat?.id ?? null;
  const drainFrames = useRef<(() => void) | null>(null);
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

  // A turn the server is still running is replayed to a connection that joins
  // it, so the row may already be on screen with everything saved so far.
  const startAssistant = useCallback((id: string, role: MessageRole) => {
    setChat((prev) => {
      if (!prev || prev.messages.some((message) => message.id === id)) return prev;
      return {
        ...prev,
        messages: [
          ...prev.messages,
          {
            id,
            chat_id: prev.id,
            role,
            content: '',
            created_at: new Date().toISOString(),
          },
        ],
      };
    });
  }, []);

  // Tool calls arrive in two frames: one when the agent starts a tool and one
  // when it finishes, so this merges a partial update into the message's trace
  // rather than replacing the record.
  const patchToolCall = useCallback(
    (messageId: string, target: ToolCallTarget, patch: Partial<ToolCallRecord>) => {
      setChat((prev) => {
        if (!prev) return prev;
        return {
          ...prev,
          messages: prev.messages.map((message) =>
            message.id === messageId ? upsertToolCall(message, target, patch) : message
          ),
        };
      });
    },
    []
  );

  // The server refused a decision this window sent: another window answered
  // the card first, or the turn moved past it. This window never learns which
  // way it went, so the card drops the outcome it assumed and the buttons that
  // offered one. The turn is untouched — its own result frame says what
  // happened to the call.
  const closeApproval = useCallback((toolCallId: string) => {
    setChat((prev) => {
      if (!prev) return prev;
      return {
        ...prev,
        messages: prev.messages.map((message) => {
          const calls = message.metadata?.tool_calls;
          if (!calls?.some((call) => call.id === toolCallId)) return message;
          return {
            ...message,
            metadata: {
              ...message.metadata,
              tool_calls: calls.map((call) =>
                call.id === toolCallId ? { ...call, approval: undefined } : call
              ),
            },
          };
        }),
      };
    });
  }, []);

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

    let disposed = false;
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
    let reconnectAttempt = 0;
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
                      message.metadata?.action_receipts?.length ||
                      message.metadata?.reasoning?.trim()
                  )
              ),
            }
          : previous
      );
    };

    const settleStoppedAssistant = (identifier: string | null): void => {
      discardEmptyAssistant(identifier);
      if (!identifier) return;
      setChat((previous) => {
        if (!previous) return previous;
        return {
          ...previous,
          messages: previous.messages.map((message) => {
            if (message.id !== identifier) return message;
            const tools = message.metadata?.tool_calls;
            const stopped = tools?.map((call) =>
              call.pending
                ? {
                    ...call,
                    pending: false,
                    success: false,
                    approval: call.approval === 'pending' ? ('denied' as const) : call.approval,
                    detail: 'Did not finish',
                  }
                : call
            );
            const hasMedia = Boolean(message.metadata?.attachments?.length);
            const content =
              message.content.trim() ||
              (stopped?.length || hasMedia ? '[Stopped before answering]' : message.content);
            return {
              ...message,
              content,
              metadata: stopped ? { ...message.metadata, tool_calls: stopped } : message.metadata,
            };
          }),
        };
      });
    };

    // Frames can beat the chat they belong to onto the wire, and every
    // case below writes to a chat that is not there yet, so hold them.
    const held: ServerMessage[] = [];
    let overflowed = false;
    const loaded = () => loadedChat.current === chatId;

    const apply = (payload: ServerMessage) => {
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
        case 'message_start': {
          // A dropped socket settles the turn locally, because from here it
          // cannot tell a reply that died from one still being written. The
          // replay says it is still being written, so take it back up.
          const settled = completed.has(payload.message_id);
          if (payload.resumed) completed.delete(payload.message_id);
          else if (settled || assistantId === payload.message_id) break;
          activeGenerationRef.current = true;
          setStreaming(true);
          generationSeen = true;
          contextEpoch.current += 1;
          setStatus(null);
          setError(null);
          assistantId = payload.message_id;
          assistantContent = '';
          assistantMetadata = undefined;
          // Replayed frames rebuild the reply from the start of the turn, so
          // only a locally settled row needs its stopped placeholder cleared.
          if (settled) upsertMessage(payload.message_id, payload.role, '');
          else startAssistant(payload.message_id, payload.role);
          break;
        }
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
        case 'tool_call': {
          const preceding = payload.reasoning ?? assistantMetadata?.reasoning;
          if (preceding) {
            assistantMetadata = { ...assistantMetadata, reasoning: undefined };
          }
          patchToolCall(payload.message_id, payload.tool_call_id, {
            name: payload.name,
            arguments: payload.arguments,
            detail: 'Running…',
            pending: true,
            reasoning: preceding,
            reason: payload.reason,
          });
          if (assistantId === payload.message_id) {
            upsertMessage(assistantId, 'assistant', assistantContent, assistantMetadata);
          }
          break;
        }
        case 'tool_approval_required':
          patchToolCall(payload.message_id, payload.tool_call_id, {
            name: payload.name,
            arguments: payload.arguments,
            detail: 'Waiting for approval…',
            pending: true,
            approval: 'pending',
            reason: payload.reason,
            preview: payload.preview,
          });
          break;
        case 'tool_approval_closed':
          closeApproval(payload.tool_call_id);
          break;
        // The turn ends here: the model asked something and the reply waits on
        // the reader, whose answer arrives as an ordinary user message rather
        // than as a decision frame of its own. The call itself already returned
        // — the card is what is waiting — so it keeps the settled state the
        // tool result gave it, which is what a reload rebuilds it as. The
        // questions are read by the schema the stored record is read by, so
        // the card this frame draws is the card that reload rebuilds. A frame
        // with no readable question on it leaves the call as the result left
        // it: a row marked as waiting with nothing to answer on is a promise
        // the card cannot keep.
        case 'question_required': {
          const questions = QuestionsSchema.safeParse(payload.questions);
          if (!questions.success || questions.data.length === 0) break;
          patchToolCall(payload.message_id, payload.tool_call_id, {
            questions: questions.data,
            detail: AWAITING_ANSWER_DETAIL,
          });
          break;
        }
        // The result settles the call for every window. One that never decided
        // it stops offering a decision it can no longer make, so nothing here
        // is left to click after another window has answered the card.
        case 'tool_result':
          patchToolCall(payload.message_id, payload.tool_call_id, {
            name: payload.name,
            success: payload.success,
            detail: payload.detail,
            duration_ms: payload.duration_ms,
            pending: false,
            approval: undefined,
          });
          if (payload.citations?.length) {
            appendCitations(payload.message_id, payload.citations);
          }
          break;
        // Each of these is read by the schema its stored counterpart is read
        // by, so the card a frame draws is the card a reload rebuilds, and a
        // frame this client cannot read leaves the call as it found it.
        case JOB_STARTED: {
          const job = JobStartedSchema.safeParse(payload.job);
          if (!job.success) break;
          patchToolCall(payload.message_id, payload.tool_call_id, { job: job.data });
          break;
        }
        case JOB_EXITED: {
          const job = JobExitedSchema.safeParse(payload.job);
          if (!job.success) break;
          patchToolCall(payload.message_id, startedBy(job.data.id), { exited: job.data });
          break;
        }
        case WAIT_STARTED: {
          const waiting = WaitingSchema.safeParse(payload.waiting);
          if (!waiting.success) break;
          patchToolCall(payload.message_id, payload.tool_call_id, { waiting: waiting.data });
          break;
        }
        case WAIT_SETTLED: {
          const settled = WaitSettledSchema.safeParse(payload.settled);
          if (!settled.success) break;
          patchToolCall(payload.message_id, settled.data.tool_call_id, { settled: settled.data });
          break;
        }
        case 'action_receipt':
          appendReceipt(payload.message_id, payload.receipt);
          break;
        case 'image':
        case 'video':
        case 'audio':
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
          settleStoppedAssistant(assistantId ?? payload.message_id);
          assistantId = null;
          contextEpoch.current += 1;
          const pendingId = pendingUserIdRef.current;
          pendingUserIdRef.current = null;
          if (pendingId) {
            supersededPendingIdsRef.current.add(pendingId);
            setChat((prev) =>
              prev
                ? {
                    ...prev,
                    messages: prev.messages.filter((message) => message.id !== pendingId),
                  }
                : prev
            );
          }
          setStatus(null);
          activeGenerationRef.current = false;
          setStreaming(false);
          break;
        }
        case 'error':
          settleStoppedAssistant(assistantId);
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

    const receive = (payload: ServerMessage) => {
      if (loaded()) {
        apply(payload);
        return;
      }
      if (overflowed) return;
      if (held.length >= MAX_HELD_FRAMES) {
        // A reply this long with no chat to put it in cannot be
        // reassembled; message_end still carries the finished turn.
        overflowed = true;
        held.length = 0;
        return;
      }
      held.push(payload);
    };

    drainFrames.current = () => {
      if (!loaded()) return;
      overflowed = false;
      for (const payload of held.splice(0)) apply(payload);
    };

    const bindSocket = (socket: WebSocket) => {
      socketRef.current = socket;
      held.length = 0;
      overflowed = false;
      // An open socket is not yet a working one: the server accepts every
      // connection and only then decides, so init is the first word that this
      // one carries a chat, and an error before it refuses the connection
      // rather than dropping one that already worked.
      let announced = false;
      let refused = false;
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
        if (payload.type === 'init') {
          announced = true;
          reconnectAttempt = 0;
        } else if (payload.type === 'error' && !announced) {
          refused = true;
        }
        receive(payload);
      };

      const invalidateContext = (): void => {
        settleStoppedAssistant(assistantId);
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
        if (socket !== socketRef.current) return;
        invalidateContext();
        assistantId = null;
        contextEpoch.current += 1;
        setError('Chat connection failed');
        setStatus(null);
        activeGenerationRef.current = false;
        setStreaming(false);
      };

      socket.onclose = () => {
        if (socket !== socketRef.current || disposed) return;
        invalidateContext();
        assistantId = null;
        contextEpoch.current += 1;
        setStatus(null);
        activeGenerationRef.current = false;
        setStreaming(false);
        if (refused) return;
        const delay =
          reconnectAttempt === 0 ? 0 : Math.min(500 * 2 ** (reconnectAttempt - 1), 8000);
        reconnectAttempt += 1;
        reconnectTimer = setTimeout(() => {
          if (disposed) return;
          bindSocket(chatsApi.createChatWebSocket(chatId));
        }, delay);
      };
    };

    bindSocket(chatsApi.createChatWebSocket(chatId));

    return () => {
      disposed = true;
      drainFrames.current = null;
      if (reconnectTimer) {
        clearTimeout(reconnectTimer);
      }
      cancelChunkFrame();
      const socket = socketRef.current;
      if (socket) {
        socket.onopen = null;
        socket.onmessage = null;
        socket.onerror = null;
        socket.onclose = null;
        socket.close();
      }
      socketRef.current = null;
    };
  }, [
    chatId,
    upsertMessage,
    startAssistant,
    applySavedUserMessage,
    patchToolCall,
    closeApproval,
    appendCitations,
    appendReceipt,
    applyTitle,
  ]);

  // The socket can connect, and the server can replay a turn already in
  // flight, before the chat those frames belong to has finished loading.
  useEffect(() => {
    if (!chat?.id) return;
    drainFrames.current?.();
  }, [chat?.id]);

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
    const pendingId = pendingUserIdRef.current ?? `pending-${crypto.randomUUID()}`;
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
    // An outcome is assumed only while the card is still open, judged on the
    // state the updater sees rather than the render that drew the buttons: a
    // result that lands in the same tick as the click keeps the row it
    // settled, and a call that already carries its result keeps saying what
    // the server said.
    setChat((prev) => {
      if (!prev) return prev;
      return {
        ...prev,
        messages: prev.messages.map((message) => {
          const calls = message.metadata?.tool_calls;
          if (!calls?.some((call) => call.id === toolCallId && call.approval === 'pending')) {
            return message;
          }
          return {
            ...message,
            metadata: {
              ...message.metadata,
              tool_calls: calls.map((call) =>
                call.id === toolCallId
                  ? {
                      ...call,
                      approval: approved ? ('approved' as const) : ('denied' as const),
                      detail: approved ? 'Approved. Running…' : 'Denied',
                      pending: approved,
                    }
                  : call
              ),
            },
          };
        }),
      };
    });
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
    setChat((prev) => {
      if (!prev || prev.id !== updated.id) {
        return prev;
      }
      const messages =
        settings.auto_approve === true
          ? prev.messages.map((message) => {
              const calls = message.metadata?.tool_calls;
              if (!calls?.some((call) => call.approval === 'pending')) {
                return message;
              }
              return {
                ...message,
                metadata: {
                  ...message.metadata,
                  tool_calls: calls.map((call) =>
                    call.approval === 'pending'
                      ? {
                          ...call,
                          approval: 'approved' as const,
                          detail: 'Approved. Running…',
                          pending: true,
                        }
                      : call
                  ),
                },
              };
            })
          : prev.messages;
      return {
        ...prev,
        ...updated,
        messages,
      };
    });
  };

  const setAgentEnabled = (enabled: boolean): Promise<void> =>
    updateAgentSettings({ agent_enabled: enabled });

  const setAutoApprove = (enabled: boolean): Promise<void> =>
    updateAgentSettings({ auto_approve: enabled });

  const setReasoningEffort = (effort: ReasoningEffort): Promise<void> =>
    updateAgentSettings({ reasoning_effort: effort });

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
    setReasoningEffort,
    setCharacter,
    clearCharacter,
    deleteMessage,
    refresh,
    updateTitle,
  };
}
