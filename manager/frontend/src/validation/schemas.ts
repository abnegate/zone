import { z } from 'zod';

// =============================================================================
// Auth Schemas
// =============================================================================

export const UserSchema = z.object({
  id: z.string(),
  email: z.string().email(),
  display_name: z.string().nullable(),
  is_active: z.boolean(),
  is_admin: z.boolean(),
  created_at: z.string(),
  updated_at: z.string(),
  last_login_at: z.string().nullable(),
});

export const AuthResponseSchema = z.object({
  access_token: z.string(),
  refresh_token: z.string(),
  expires_in: z.number(),
  user: UserSchema,
  roles: z.array(z.string()),
  permissions: z.array(z.string()),
});

export const LoginRequestSchema = z.object({
  email: z.string().email('Invalid email address'),
  password: z.string().min(1, 'Password is required'),
});

export const RegisterRequestSchema = z.object({
  email: z.string().email('Invalid email address'),
  password: z.string().min(8, 'Password must be at least 8 characters'),
  display_name: z.string().optional(),
});

export const JwtPayloadSchema = z.object({
  sub: z.string(),
  email: z.string().email(),
  roles: z.array(z.string()),
  permissions: z.array(z.string()),
  iat: z.number(),
  exp: z.number(),
  jti: z.string(),
});

// =============================================================================
// Organization Schemas
// =============================================================================

export const OrganizationSchema = z.object({
  id: z.string(),
  name: z.string(),
  slug: z.string(),
  description: z.string().nullable(),
  is_active: z.boolean(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const CreateOrganizationRequestSchema = z.object({
  name: z.string().min(1, 'Name is required'),
  slug: z.string().min(1, 'Slug is required'),
  description: z.string().optional(),
});

export const UpdateOrganizationRequestSchema = z.object({
  name: z.string().min(1).optional(),
  slug: z.string().min(1).optional(),
  description: z.string().optional(),
  is_active: z.boolean().optional(),
});

// =============================================================================
// Workspace Schemas
// =============================================================================

export const WorkspaceSchema = z.object({
  id: z.string(),
  organization_id: z.string(),
  name: z.string(),
  slug: z.string(),
  description: z.string().nullable(),
  is_active: z.boolean(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const CreateWorkspaceRequestSchema = z.object({
  name: z.string().min(1, 'Name is required'),
  slug: z.string().min(1, 'Slug is required'),
  description: z.string().optional(),
});

export const UpdateWorkspaceRequestSchema = z.object({
  name: z.string().min(1).optional(),
  slug: z.string().min(1).optional(),
  description: z.string().optional(),
  is_active: z.boolean().optional(),
});

// =============================================================================
// Model Schemas
// =============================================================================

export const InstalledModelSchema = z.object({
  name: z.string(),
  size: z.number(),
  modified_at: z.string(),
  details: z
    .object({
      description: z.string().optional(),
      family: z.string().optional(),
    })
    .optional(),
});

export const BrowseModelSchema = z.object({
  id: z.string(),
  name: z.string(),
  description: z.string(),
  downloads: z.number(),
  tags: z.array(z.string()),
  install_name: z.string().nullable().optional(),
  author: z.string().nullable().optional(),
  likes: z.number().nullable().optional(),
  last_modified: z.string().nullable().optional(),
  url: z.string().nullable().optional(),
});

export const ModelSourceSchema = z.enum(['ollama', 'huggingface', 'modelscope']);

export const ModelsResponseSchema = z.object({
  models: z.array(InstalledModelSchema),
});

export const BrowseResponseSchema = z.object({
  source: ModelSourceSchema,
  models: z.array(BrowseModelSchema),
  total: z.number().nullable().optional(),
  has_more: z.boolean(),
});

export const PullProgressSchema = z.object({
  type: z.enum(['progress', 'step', 'complete', 'error', 'authenticated']),
  status: z.string().optional(),
  percent: z.number().optional(),
  completed: z.number().optional(),
  total: z.number().optional(),
  message: z.string().optional(),
  success: z.boolean().optional(),
});

// =============================================================================
// Chat Schemas
// =============================================================================

export const MessageRoleSchema = z.enum(['user', 'assistant', 'system']);

export const MessageSchema = z.object({
  id: z.string(),
  chat_id: z.string(),
  role: MessageRoleSchema,
  content: z.string(),
  created_at: z.string(),
});

export const ChatSchema = z.object({
  id: z.string(),
  title: z.string(),
  model_name: z.string(),
  created_at: z.string(),
  updated_at: z.string(),
  archived: z.boolean(),
});

export const ChatWithMessagesSchema = ChatSchema.extend({
  messages: z.array(MessageSchema),
});

export const CreateChatRequestSchema = z.object({
  model_name: z.string().min(1, 'Model is required'),
  first_message: z.string().optional(),
});

export const SendMessageRequestSchema = z.object({
  content: z.string().min(1, 'Message cannot be empty'),
});

export const ChatsResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  chats: z.array(ChatSchema),
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
// Source Config Schemas
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
// Project Schemas
// =============================================================================

export const ProjectStatusSchema = z.enum(['active', 'on_hold', 'cancelled']);

export const ProjectSchema = z.object({
  id: z.string(),
  name: z.string(),
  description: z.string().nullable(),
  status: ProjectStatusSchema,
  github_repo_url: z.string().nullable(),
  source_id: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const CreateProjectRequestSchema = z.object({
  name: z.string().min(1, 'Name is required'),
  description: z.string().optional(),
  status: ProjectStatusSchema.optional(),
  github_repo_url: z.string().optional(),
  source_id: z.string().optional(),
});

export const UpdateProjectRequestSchema = z.object({
  name: z.string().min(1).optional(),
  description: z.string().optional(),
  status: ProjectStatusSchema.optional(),
  github_repo_url: z.string().optional(),
  source_id: z.string().optional(),
});

export const ProjectsResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  projects: z.array(ProjectSchema),
});

export const ProjectResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  project: ProjectSchema,
});

// =============================================================================
// Task Schemas
// =============================================================================

export const TaskStatusSchema = z.enum([
  'created',
  'queued',
  'in_progress',
  'blocked',
  'review',
  'complete',
]);
export const RunStatusSchema = z.enum(['running', 'completed', 'failed', 'cancelled']);
export const LogLevelSchema = z.enum(['debug', 'info', 'warning', 'error']);

export const TaskSchema = z.object({
  id: z.string(),
  project_id: z.string(),
  title: z.string(),
  description: z.string(),
  acceptance_criteria: z.string().nullable(),
  status: TaskStatusSchema,
  priority: z.number(),
  model_name: z.string().nullable(),
  dependencies: z.array(z.string()),
  created_at: z.string(),
  updated_at: z.string(),
  started_at: z.string().nullable(),
  completed_at: z.string().nullable(),
  is_agentic: z.boolean(),
  github_repo_url: z.string().nullable(),
  source_id: z.string().nullable(),
  source_ids: z.array(z.string()),
  queued_at: z.string().nullable(),
  worker_id: z.string().nullable(),
});

export const TaskRunSchema = z.object({
  id: z.string(),
  task_id: z.string(),
  status: RunStatusSchema,
  current_phase: z.string().nullable(),
  progress_percent: z.number(),
  error_message: z.string().nullable(),
  started_at: z.string(),
  completed_at: z.string().nullable(),
});

export const TaskRunLogSchema = z.object({
  id: z.string(),
  run_id: z.string(),
  phase: z.string(),
  agent_type: z.string(),
  level: LogLevelSchema,
  message: z.string(),
  created_at: z.string(),
});

export const CreateTaskRequestSchema = z.object({
  project_id: z.string().min(1, 'Project is required'),
  title: z.string().min(1, 'Title is required'),
  description: z.string().min(1, 'Description is required'),
  acceptance_criteria: z.string().optional(),
  priority: z.number().optional(),
  model_name: z.string().optional(),
  dependencies: z.array(z.string()).optional(),
  is_agentic: z.boolean().optional(),
  github_repo_url: z.string().optional(),
  source_id: z.string().optional(),
  source_ids: z.array(z.string()).optional(),
});

export const UpdateTaskRequestSchema = z.object({
  title: z.string().min(1).optional(),
  description: z.string().min(1).optional(),
  acceptance_criteria: z.string().optional(),
  status: TaskStatusSchema.optional(),
  priority: z.number().optional(),
  model_name: z.string().optional(),
  dependencies: z.array(z.string()).optional(),
  is_agentic: z.boolean().optional(),
  github_repo_url: z.string().optional(),
  source_id: z.string().optional(),
  source_ids: z.array(z.string()).optional(),
});

export const TasksResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  tasks: z.array(TaskSchema),
});

export const TaskResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  task: TaskSchema,
});

export const TaskRunsResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  runs: z.array(TaskRunSchema),
});

export const TaskRunResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  run: TaskRunSchema,
});

export const TaskRunLogsResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  logs: z.array(TaskRunLogSchema),
});

export const TaskProgressMessageSchema = z.object({
  type: z.enum(['phase_started', 'phase_completed', 'log', 'complete', 'error']),
  run_id: z.string(),
  phase: z.string().optional(),
  progress_percent: z.number().optional(),
  message: z.string().optional(),
  agent_type: z.string().optional(),
  log_level: z.string().optional(),
  success: z.boolean().optional(),
  error: z.string().optional(),
});

// =============================================================================
// Workspace Theme Schemas
// =============================================================================

export const FontFamilySchema = z.enum([
  'system',
  'inter',
  'roboto',
  'open-sans',
  'lato',
  'nunito',
]);
export const BorderRadiusSchema = z.enum(['none', 'small', 'medium', 'large']);

const hexColorRegex = /^#([A-Fa-f0-9]{6}|[A-Fa-f0-9]{3})$/;

export const WorkspaceThemeSchema = z.object({
  id: z.string(),
  workspace_id: z.string(),
  primary_color_light: z.string().regex(hexColorRegex, 'Invalid hex color'),
  secondary_color_light: z.string().regex(hexColorRegex, 'Invalid hex color'),
  primary_color_dark: z.string().regex(hexColorRegex, 'Invalid hex color'),
  secondary_color_dark: z.string().regex(hexColorRegex, 'Invalid hex color'),
  font_family: FontFamilySchema,
  font_size_base: z.string(),
  border_radius: BorderRadiusSchema,
  created_at: z.string(),
  updated_at: z.string(),
});

export const UpdateWorkspaceThemeRequestSchema = z.object({
  primary_color_light: z.string().regex(hexColorRegex, 'Invalid hex color').optional(),
  secondary_color_light: z.string().regex(hexColorRegex, 'Invalid hex color').optional(),
  primary_color_dark: z.string().regex(hexColorRegex, 'Invalid hex color').optional(),
  secondary_color_dark: z.string().regex(hexColorRegex, 'Invalid hex color').optional(),
  font_family: FontFamilySchema.optional(),
  font_size_base: z.string().optional(),
  border_radius: BorderRadiusSchema.optional(),
});

export const WorkspaceThemeResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  theme: WorkspaceThemeSchema,
});

// =============================================================================
// Organizations Response Schemas
// =============================================================================

export const OrganizationsResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  organizations: z.array(OrganizationSchema),
});

export const OrganizationResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  organization: OrganizationSchema,
});

// =============================================================================
// Workspaces Response Schemas
// =============================================================================

export const WorkspacesResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  workspaces: z.array(WorkspaceSchema),
});

export const WorkspaceResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  workspace: WorkspaceSchema,
});

// =============================================================================
// Content Schemas
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

export type UserZ = z.infer<typeof UserSchema>;
export type AuthResponseZ = z.infer<typeof AuthResponseSchema>;
export type LoginRequestZ = z.infer<typeof LoginRequestSchema>;
export type RegisterRequestZ = z.infer<typeof RegisterRequestSchema>;
export type JwtPayloadZ = z.infer<typeof JwtPayloadSchema>;
export type OrganizationZ = z.infer<typeof OrganizationSchema>;
export type WorkspaceZ = z.infer<typeof WorkspaceSchema>;
export type ChatZ = z.infer<typeof ChatSchema>;
export type MessageZ = z.infer<typeof MessageSchema>;
export type SourceZ = z.infer<typeof SourceSchema>;
export type ProjectZ = z.infer<typeof ProjectSchema>;
export type TaskZ = z.infer<typeof TaskSchema>;
export type TaskRunZ = z.infer<typeof TaskRunSchema>;
export type WorkspaceThemeZ = z.infer<typeof WorkspaceThemeSchema>;

// Response type exports for API client
export type ChatsResponse = z.infer<typeof ChatsResponseSchema>;
export type ChatResponse = z.infer<typeof ChatResponseSchema>;
export type MessagesResponse = z.infer<typeof MessagesResponseSchema>;
export type MessageResponse = z.infer<typeof MessageResponseSchema>;
export type ProjectsResponse = z.infer<typeof ProjectsResponseSchema>;
export type ProjectResponse = z.infer<typeof ProjectResponseSchema>;
export type SourcesResponse = z.infer<typeof SourcesResponseSchema>;
export type SourceResponse = z.infer<typeof SourceResponseSchema>;
export type SourceTypesResponse = z.infer<typeof SourceTypesResponseSchema>;
export type TasksResponse = z.infer<typeof TasksResponseSchema>;
export type TaskResponse = z.infer<typeof TaskResponseSchema>;
export type TaskRunsResponse = z.infer<typeof TaskRunsResponseSchema>;
export type TaskRunResponse = z.infer<typeof TaskRunResponseSchema>;
export type TaskRunLogsResponse = z.infer<typeof TaskRunLogsResponseSchema>;
export type OrganizationsResponse = z.infer<typeof OrganizationsResponseSchema>;
export type OrganizationResponse = z.infer<typeof OrganizationResponseSchema>;
export type WorkspacesResponse = z.infer<typeof WorkspacesResponseSchema>;
export type WorkspaceResponse = z.infer<typeof WorkspaceResponseSchema>;
export type WorkspaceThemeResponse = z.infer<typeof WorkspaceThemeResponseSchema>;
export type SourceVerifyResponse = z.infer<typeof SourceVerifyResponseSchema>;
