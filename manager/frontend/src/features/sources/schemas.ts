/**
 * Source Schemas
 * Zod validation schemas for all source-related types.
 */

import { z } from 'zod';

// =============================================================================
// Source Type Schemas
// =============================================================================

export const SourceCategorySchema = z.enum(['file', 'calendar', 'mail', 'chat', 'web', 'text']);
export const SourceTypeSchema = z.enum([
  'github',
  'gitlab',
  'filesystem',
  'ical',
  'imap',
  'discord',
  'slack',
  'web',
  'text',
]);

// =============================================================================
// Source Config Schemas
// =============================================================================

export const GitHubConfigSchema = z.object({
  owner: z.string().min(1, 'Owner is required'),
  repo: z.string().min(1, 'Repository is required'),
  branch: z.string().optional(),
  base_path: z.string().optional(),
});

export const GitLabConfigSchema = z.object({
  project_id: z.string().min(1, 'Project ID is required'),
  host: z.string().optional(),
  branch: z.string().optional(),
  base_path: z.string().optional(),
});

export const FilesystemConfigSchema = z.object({
  base_path: z.string().min(1, 'Base path is required'),
  allow_writes: z.boolean().optional(),
});

export const ICalConfigSchema = z.object({
  url: z.string().url('Invalid URL'),
  refresh_interval: z.number().optional(),
});

export const IMAPConfigSchema = z.object({
  host: z.string().min(1, 'Host is required'),
  port: z.number().optional(),
  username: z.string().min(1, 'Username is required'),
  use_ssl: z.boolean().optional(),
  folder: z.string().optional(),
});

export const DiscordConfigSchema = z.object({
  server_id: z.string().min(1, 'Server ID is required'),
  channel_ids: z.array(z.string()).optional(),
});

export const SlackConfigSchema = z.object({
  workspace_id: z.string().min(1, 'Workspace ID is required'),
  channel_ids: z.array(z.string()).optional(),
});

export const WebConfigSchema = z.object({
  url: z.string().url('Invalid URL'),
  headers: z.record(z.string()).optional(),
});

export const TextConfigSchema = z.object({
  content: z.string().min(1, 'Content is required'),
  label: z.string().optional(),
});

export const SourceConfigSchema = z.union([
  GitHubConfigSchema,
  GitLabConfigSchema,
  FilesystemConfigSchema,
  ICalConfigSchema,
  IMAPConfigSchema,
  DiscordConfigSchema,
  SlackConfigSchema,
  WebConfigSchema,
  TextConfigSchema,
]);

// =============================================================================
// Main Source Schema
// =============================================================================

export const SourceSchema = z.object({
  id: z.string(),
  name: z.string(),
  source_type: SourceTypeSchema,
  category: SourceCategorySchema,
  config: SourceConfigSchema,
  description: z.string().nullable(),
  url: z.string(),
  is_active: z.boolean(),
  last_verified_at: z.string().nullable(),
  last_error: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
});

// =============================================================================
// Request/Response Schemas
// =============================================================================

export const CreateSourceRequestSchema = z.object({
  name: z.string().min(1, 'Name is required'),
  source_type: SourceTypeSchema,
  config: SourceConfigSchema,
  credentials: z.string().optional(),
  description: z.string().optional(),
});

export const UpdateSourceRequestSchema = z.object({
  name: z.string().min(1).optional(),
  config: SourceConfigSchema.optional(),
  credentials: z.string().optional(),
  description: z.string().optional(),
  is_active: z.boolean().optional(),
});

export const SourcesResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  sources: z.array(SourceSchema),
});

export const SourceResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  source: SourceSchema,
});

export const SourceVerifyResponseSchema = z.object({
  success: z.boolean(),
  message: z.string(),
  item_count: z.number().optional(),
});

export const SourceTypeInfoSchema = z.object({
  id: SourceTypeSchema,
  name: z.string(),
  category: SourceCategorySchema,
  enabled: z.boolean(),
});

export const SourceTypesResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  types: z.array(SourceTypeInfoSchema),
});

// =============================================================================
// Content Metadata Schemas
// =============================================================================

export const FileMetadataSchema = z.object({
  type: z.literal('file'),
  path: z.string(),
  size: z.number(),
  sha: z.string().nullable(),
  is_directory: z.boolean(),
});

export const CalendarMetadataSchema = z.object({
  type: z.literal('calendar'),
  start_time: z.string(),
  end_time: z.string(),
  location: z.string().nullable(),
  attendees: z.array(z.string()),
  recurrence: z.string().nullable(),
  all_day: z.boolean(),
});

export const MailMetadataSchema = z.object({
  type: z.literal('mail'),
  from: z.string(),
  to: z.array(z.string()),
  cc: z.array(z.string()),
  subject: z.string(),
  thread_id: z.string().nullable(),
  attachments: z.array(z.string()),
  is_read: z.boolean(),
});

export const ChatMetadataSchema = z.object({
  type: z.literal('chat'),
  channel_id: z.string(),
  channel_name: z.string().nullable(),
  author_id: z.string(),
  author_name: z.string(),
  thread_id: z.string().nullable(),
  reactions: z.array(z.string()),
});

export const WebMetadataSchema = z.object({
  type: z.literal('web'),
  status_code: z.number(),
  headers: z.record(z.string()),
  fetched_at: z.string(),
});

export const TextMetadataSchema = z.object({
  type: z.literal('text'),
  label: z.string().nullable(),
});

export const ContentMetadataSchema = z.discriminatedUnion('type', [
  FileMetadataSchema,
  CalendarMetadataSchema,
  MailMetadataSchema,
  ChatMetadataSchema,
  WebMetadataSchema,
  TextMetadataSchema,
]);

export const ContentItemSchema = z.object({
  id: z.string(),
  source_id: z.string(),
  category: SourceCategorySchema,
  title: z.string(),
  content: z.string(),
  content_type: z.string(),
  timestamp: z.string().nullable(),
  url: z.string().nullable(),
  metadata: ContentMetadataSchema,
});

export const ContentResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  items: z.array(ContentItemSchema),
  total: z.number(),
  has_more: z.boolean(),
});

// =============================================================================
// Type Exports (inferred from schemas)
// =============================================================================

export type SourceZ = z.infer<typeof SourceSchema>;
export type SourceCategoryZ = z.infer<typeof SourceCategorySchema>;
export type SourceTypeZ = z.infer<typeof SourceTypeSchema>;
export type CreateSourceRequestZ = z.infer<typeof CreateSourceRequestSchema>;
export type UpdateSourceRequestZ = z.infer<typeof UpdateSourceRequestSchema>;
export type SourcesResponse = z.infer<typeof SourcesResponseSchema>;
export type SourceResponse = z.infer<typeof SourceResponseSchema>;
export type SourceVerifyResponse = z.infer<typeof SourceVerifyResponseSchema>;
export type SourceTypesResponse = z.infer<typeof SourceTypesResponseSchema>;
export type ContentItemZ = z.infer<typeof ContentItemSchema>;
export type ContentResponse = z.infer<typeof ContentResponseSchema>;
