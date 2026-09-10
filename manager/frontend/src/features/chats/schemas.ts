import { z } from 'zod';

export const MessageRoleSchema = z.enum(['user', 'assistant', 'system']);

export const MessageAttachmentSchema = z.object({
  name: z.string(),
  mime: z.string(),
  url: z.string(),
});

/**
 * The model's stated reason for a side-effecting call.
 *
 * Optional, because every record stored before the field existed has to keep
 * parsing, and these objects are not passthrough: a required field here would
 * make older entries non-conforming, and `tolerantArray` drops those, silently
 * deleting the history it exists to protect. Unreadable values fall back to
 * absent for the same reason — a bad reason costs the reason, never the row.
 */
const statedReason = z.string().optional().catch(undefined);

export const ToolCallRecordSchema = z.object({
  id: z.string(),
  name: z.string(),
  arguments: z.string(),
  success: z.boolean(),
  detail: z.string(),
  duration_ms: z.number(),
  reasoning: z.string().optional(),
  reason: statedReason,
});

/**
 * Keeps the elements that validate and drops the ones that do not.
 *
 * A citation, tool call or receipt the backend has learned to emit and this
 * client has not must cost that one chip, never the message it arrived on.
 * These arrays sit inside the chat response, so a single unrecognised value
 * would otherwise throw for the whole GET and leave the chat permanently
 * unopenable, since the value is persisted in messages.metadata.
 */
function tolerantArray<T extends z.ZodTypeAny>(element: T) {
  return z.array(z.unknown()).transform((items) =>
    items
      .map((item) => element.safeParse(item))
      .filter((result): result is { success: true; data: z.infer<T> } => result.success)
      .map((result) => result.data)
  );
}

export const CitationSchema = z.object({
  kind: z.enum([
    'github_build',
    'github_deployment',
    'github_issue',
    'github_file',
    'workspace_document',
    'behavioral_verification',
  ]),
  title: z.string(),
  url: z.string(),
  revision: z.string().nullish(),
  observed_at: z.string(),
  complete: z.boolean(),
  outcome: z.enum(['success', 'failure', 'pending', 'incomplete', 'observed']),
  // Absent means a citation stored before the field existed, which the server
  // also reads as server-proven. A value this client does not recognise is
  // demoted to a claim instead: an unreadable provenance must never be
  // presented as proof.
  provenance: z
    .enum(['server_execution', 'model_asserted'])
    .catch('model_asserted')
    .default('server_execution'),
  note: z.string().nullish(),
});

export const ActionTargetSchema = z.enum(['task', 'document', 'message', 'reminder']);

export const ActionReceiptSchema = z.object({
  id: z.string(),
  action: z.string(),
  target_type: ActionTargetSchema,
  target_id: z.string(),
  target_label: z.string(),
  actor_id: z.string(),
  actor_name: z.string(),
  occurred_at: z.string(),
  success: z.boolean(),
  outcome: z.string(),
  href: z.string(),
  reason: statedReason,
});

export const MessageMetadataSchema = z
  .object({
    attachments: z.array(MessageAttachmentSchema).optional(),
    tool_calls: tolerantArray(ToolCallRecordSchema).optional(),
    citations: tolerantArray(CitationSchema).optional(),
    action_receipts: tolerantArray(ActionReceiptSchema).optional(),
    web_search: z.boolean().optional(),
    reasoning: z.string().optional(),
  })
  .passthrough();

export const MessageSchema = z.object({
  id: z.string(),
  chat_id: z.string(),
  role: MessageRoleSchema,
  content: z.string(),
  created_at: z.string(),
  metadata: MessageMetadataSchema.nullish(),
});

export const ChatCharacterSchema = z
  .object({
    name: z.string(),
    description: z.string().nullish(),
    personality: z.string().nullish(),
    scenario: z.string().nullish(),
    first_mes: z.string().nullish(),
    mes_example: z.string().nullish(),
    system_prompt: z.string().nullish(),
    post_history_instructions: z.string().nullish(),
    stop_sequences: z.array(z.string()).optional(),
    source_name: z.string().nullish(),
  })
  .passthrough();

export const ChatSchema = z.object({
  id: z.string(),
  title: z.string(),
  model_name: z.string(),
  created_at: z.string(),
  updated_at: z.string(),
  archived: z.boolean(),
  // Servers predating agentic chat omit this; treat those chats as plain.
  agent_enabled: z.boolean().default(false),
  auto_approve: z.boolean().default(false),
  reasoning: z.boolean().nullish(),
  reasoning_effort: z.enum(['auto', 'off', 'low', 'medium', 'high']).default('auto'),
  character: ChatCharacterSchema.nullish(),
  tools: z.boolean().nullish(),
  needs_character: z.boolean().nullish(),
});

const tokenCount = z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER);
export const ContextUsageSchema = z
  .object({
    model: z.string(),
    used: tokenCount,
    limit: tokenCount.nullable(),
    reserved: tokenCount,
    threshold: tokenCount.nullable(),
    remaining: tokenCount.nullable(),
    estimated: z.boolean(),
    incomplete: z.boolean(),
    source: z.enum(['runtime', 'configured', 'provider', 'unknown']),
    status: z.enum(['ready', 'compacting', 'compacted', 'unavailable', 'blocked']),
    breakdown: z.object({
      instructions: tokenCount,
      conversation: tokenCount,
      tools: tokenCount,
      results: tokenCount,
      summary: tokenCount,
      attachments: tokenCount.nullable(),
      overhead: tokenCount,
    }),
    revision: tokenCount,
    compacted_messages: tokenCount,
    updated_at: z.string(),
    reason: z.string().nullish(),
  })
  .refine((usage) => usage.breakdown.attachments !== null || usage.incomplete);
export const ContextResponseSchema = z.object({ context: ContextUsageSchema });

export const ChatWithMessagesSchema = ChatSchema.extend({
  messages: z.array(MessageSchema),
  context: ContextUsageSchema.nullish().catch(null),
});

export const CreateChatRequestSchema = z.object({
  workspace_id: z.string().min(1, 'Workspace is required'),
  title: z.string().min(1, 'Title is required'),
  model_name: z.string().min(1, 'Model is required'),
  first_message: z.string().optional(),
  agent_enabled: z.boolean().optional(),
  auto_approve: z.boolean().optional(),
  reasoning_effort: z.enum(['auto', 'off', 'low', 'medium', 'high']).optional(),
});

export const SendMessageRequestSchema = z.object({
  content: z.string().min(1, 'Message cannot be empty'),
});

export const ChatsResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  chats: z.array(ChatSchema).default([]),
});

export const ChatResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  chat: ChatWithMessagesSchema,
});

export const MessagesResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  messages: z.array(MessageSchema),
});

export const MessageResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  message: MessageSchema,
});

export const ChatSearchResultSchema = z.object({
  message_id: z.string(),
  chat_id: z.string(),
  chat_title: z.string(),
  content: z.string(),
  snippet: z.string(),
  relevance_score: z.number(),
  created_at: z.string(),
});

export const ChatSearchResponseSchema = z.object({
  results: z.array(ChatSearchResultSchema),
  total: z.number(),
});
