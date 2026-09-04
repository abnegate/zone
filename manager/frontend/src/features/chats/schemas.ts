import { z } from 'zod';

// =============================================================================
// Chat Schemas
// =============================================================================

export const MessageRoleSchema = z.enum(['user', 'assistant', 'system']);

export const MessageAttachmentSchema = z.object({
  name: z.string(),
  mime: z.string(),
  url: z.string(),
});

export const ToolCallRecordSchema = z.object({
  id: z.string(),
  name: z.string(),
  arguments: z.string(),
  success: z.boolean(),
  detail: z.string(),
  duration_ms: z.number(),
});

export const CitationSchema = z.object({
  kind: z.enum([
    'github_build',
    'github_deployment',
    'github_issue',
    'github_file',
    'workspace_document',
  ]),
  title: z.string(),
  url: z.string(),
  revision: z.string().nullish(),
  observed_at: z.string(),
  complete: z.boolean(),
  outcome: z.enum(['success', 'failure', 'pending', 'incomplete', 'observed']),
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
});

export const MessageMetadataSchema = z
  .object({
    attachments: z.array(MessageAttachmentSchema).optional(),
    tool_calls: z.array(ToolCallRecordSchema).optional(),
    citations: z.array(CitationSchema).optional(),
    action_receipts: z.array(ActionReceiptSchema).optional(),
    web_search: z.boolean().optional(),
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
});

export const ChatWithMessagesSchema = ChatSchema.extend({
  messages: z.array(MessageSchema),
});

export const CreateChatRequestSchema = z.object({
  workspace_id: z.string().min(1, 'Workspace is required'),
  title: z.string().min(1, 'Title is required'),
  model_name: z.string().min(1, 'Model is required'),
  first_message: z.string().optional(),
  agent_enabled: z.boolean().optional(),
  auto_approve: z.boolean().optional(),
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

// =============================================================================
// Chat Search Schemas
// =============================================================================

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
