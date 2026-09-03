import { z } from 'zod';
import type {
  AuthResponseSchema,
  JwtPayloadSchema,
  LoginRequestSchema,
  RegisterRequestSchema,
  UserSchema,
} from '../features/auth/schemas';
import type {
  ChatResponseSchema,
  ChatSchema,
  ChatsResponseSchema,
  MessageResponseSchema,
  MessageSchema,
  MessagesResponseSchema,
} from '../features/chats/schemas';
import type {
  SourceResponseSchema,
  SourceSchema,
  SourcesResponseSchema,
  SourceTypesResponseSchema,
  SourceVerifyResponseSchema,
} from '../features/sources/schemas';
import type {
  TaskResponseSchema,
  TaskRunLogsResponseSchema,
  TaskRunResponseSchema,
  TaskRunSchema,
  TaskRunsResponseSchema,
  TaskSchema,
  TasksResponseSchema,
} from '../features/tasks/schemas';

// =============================================================================
// Auth Schemas - now re-exported from features/auth
// =============================================================================

export {
  AuthResponseSchema,
  ForgotPasswordResponseSchema,
  ForgotPasswordSchema,
  JwtPayloadSchema,
  LoginRequestSchema,
  OrgRoleSchema,
  RegisterRequestSchema,
  ResendVerificationRequestSchema,
  ResendVerificationResponseSchema,
  ResetPasswordResponseSchema,
  ResetPasswordSchema,
  UserSchema,
  VerifyEmailRequestSchema,
  VerifyEmailResponseSchema,
  WorkspaceRoleSchema,
} from '../features/auth/schemas';

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

export {
  BrowseModelSchema,
  BrowseResponseSchema,
  InstalledModelSchema,
  ModelSourceSchema,
  ModelsResponseSchema,
  PullProgressSchema,
} from '../features/models/schemas';

// =============================================================================
// Chat Schemas
// =============================================================================

export {
  ChatResponseSchema,
  ChatSchema,
  ChatSearchResponseSchema,
  ChatSearchResultSchema,
  ChatsResponseSchema,
  ChatWithMessagesSchema,
  CreateChatRequestSchema,
  MessageResponseSchema,
  MessageRoleSchema,
  MessageSchema,
  MessagesResponseSchema,
  SendMessageRequestSchema,
} from '../features/chats/schemas';

// =============================================================================
// Source Schemas - re-exported from features/sources
// =============================================================================

export {
  CalendarMetadataSchema,
  ChatMetadataSchema,
  ContentItemSchema,
  ContentMetadataSchema,
  ContentResponseSchema,
  CreateSourceRequestSchema,
  DiscordConfigSchema,
  FileMetadataSchema,
  FilesystemConfigSchema,
  GitHubConfigSchema,
  GitLabConfigSchema,
  ICalConfigSchema,
  IMAPConfigSchema,
  MailMetadataSchema,
  SlackConfigSchema,
  SourceCategorySchema,
  SourceConfigSchema,
  SourceResponseSchema,
  SourceSchema,
  SourcesResponseSchema,
  SourceTypeInfoSchema,
  SourceTypeSchema,
  SourceTypesResponseSchema,
  SourceVerifyResponseSchema,
  TextConfigSchema,
  TextMetadataSchema,
  UpdateSourceRequestSchema,
  WebConfigSchema,
  WebMetadataSchema,
} from '../features/sources/schemas';

// =============================================================================
// Project Schemas
// =============================================================================

export {
  CreateProjectRequestSchema,
  CreateSyncConfigRequestSchema,
  ProjectResponseSchema,
  ProjectSchema,
  ProjectStatusSchema,
  ProjectsResponseSchema,
  SyncConfigResponseSchema,
  SyncConfigSchema,
  SyncConfigsResponseSchema,
  SyncDirectionSchema,
  SyncProviderSchema,
  UpdateProjectRequestSchema,
} from '../features/projects/schemas';

// =============================================================================
// Task Schemas - now re-exported from features/tasks
// =============================================================================

export {
  CreateTaskRequestSchema,
  LogLevelSchema,
  PrStatusSchema,
  RunStatusSchema,
  TaskProgressMessageSchema,
  TaskResponseSchema,
  TaskRunLogSchema,
  TaskRunLogsResponseSchema,
  TaskRunResponseSchema,
  TaskRunSchema,
  TaskRunsResponseSchema,
  TaskSchema,
  TaskStatusSchema,
  TasksResponseSchema,
  UpdateTaskRequestSchema,
} from '../features/tasks/schemas';

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
// (Content schemas re-exported from features/sources above)

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
export type TaskZ = z.infer<typeof TaskSchema>;
export type TaskRunZ = z.infer<typeof TaskRunSchema>;
export type WorkspaceThemeZ = z.infer<typeof WorkspaceThemeSchema>;

// Response type exports for API client
export type ChatsResponse = z.infer<typeof ChatsResponseSchema>;
export type ChatResponse = z.infer<typeof ChatResponseSchema>;
export type MessagesResponse = z.infer<typeof MessagesResponseSchema>;
export type MessageResponse = z.infer<typeof MessageResponseSchema>;
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

// =============================================================================
// AI Settings Schemas
// =============================================================================

export const AiProviderSchema = z.enum(['self_hosted', 'openai', 'anthropic', 'bedrock']);

export const AiSettingsSchema = z.object({
  provider: AiProviderSchema,
  has_litellm_key: z.boolean(),
  litellm_host: z.string().nullable(),
  has_openai_api_key: z.boolean(),
  openai_base_url: z.string().nullable(),
  has_anthropic_api_key: z.boolean(),
  anthropic_base_url: z.string().nullable(),
  bedrock_region: z.string().nullable(),
  bedrock_use_iam_role: z.boolean(),
  has_bedrock_credentials: z.boolean(),
  model_fast: z.string().nullable(),
  model_reasoning: z.string().nullable(),
  model_embedding: z.string().nullable(),
});

export const UpdateAiSettingsRequestSchema = z.object({
  provider: AiProviderSchema.optional(),
  litellm_host: z.string().optional(),
  litellm_key: z.string().optional(),
  openai_api_key: z.string().optional(),
  openai_base_url: z.string().optional(),
  anthropic_api_key: z.string().optional(),
  anthropic_base_url: z.string().optional(),
  bedrock_region: z.string().optional(),
  bedrock_access_key: z.string().optional(),
  bedrock_secret_key: z.string().optional(),
  bedrock_use_iam_role: z.boolean().optional(),
  model_fast: z.string().optional(),
  model_reasoning: z.string().optional(),
  model_embedding: z.string().optional(),
});

export const AiSettingsResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  provider: AiProviderSchema,
  has_litellm_key: z.boolean(),
  litellm_host: z.string().nullable(),
  has_openai_api_key: z.boolean(),
  openai_base_url: z.string().nullable(),
  has_anthropic_api_key: z.boolean(),
  anthropic_base_url: z.string().nullable(),
  bedrock_region: z.string().nullable(),
  bedrock_use_iam_role: z.boolean(),
  has_bedrock_credentials: z.boolean(),
  model_fast: z.string().nullable(),
  model_reasoning: z.string().nullable(),
  model_embedding: z.string().nullable(),
});

export type AiSettingsZ = z.infer<typeof AiSettingsSchema>;
export type AiSettingsResponse = z.infer<typeof AiSettingsResponseSchema>;

// =============================================================================
// Session Schemas
// =============================================================================

export type { SessionsResponse, SessionZ } from '../features/auth/schemas';
export { SessionSchema, SessionsResponseSchema } from '../features/auth/schemas';

// =============================================================================
// Organization Member Schemas

export type { OrgRoleZ } from '../features/auth/schemas';
export type {
  OrganizationMemberZ,
  OrgMembersResponse,
} from '../features/settings/organization/schemas';
export {
  AddOrgMemberRequestSchema,
  OrganizationMemberSchema,
  OrgMembersResponseSchema,
  UpdateOrgMemberRequestSchema,
} from '../features/settings/organization/schemas';

// =============================================================================
// Workspace Member Schemas

export type { WorkspaceRoleZ } from '../features/auth/schemas';
export type {
  WorkspaceMembersResponse,
  WorkspaceMemberZ,
} from '../features/settings/workspace/schemas';
export {
  AddWorkspaceMemberRequestSchema,
  UpdateWorkspaceMemberRequestSchema,
  WorkspaceMemberSchema,
  WorkspaceMembersResponseSchema,
} from '../features/settings/workspace/schemas';

// =============================================================================
// Invitation Schemas - now re-exported from features/settings

export type { InvitationDetailsZ } from '../features/auth/schemas';
export { InvitationDetailsSchema } from '../features/auth/schemas';
export type {
  InvitationsResponse,
  InvitationZ,
} from '../features/settings/organization/schemas';
export {
  CreateInvitationRequestSchema,
  InvitationSchema,
  InvitationsResponseSchema,
} from '../features/settings/organization/schemas';

// =============================================================================
// Billing & Usage Schemas
// =============================================================================

export const SubscriptionStatusSchema = z.enum(['active', 'canceled', 'past_due', 'trialing']);

export const PlanLimitsSchema = z.object({
  max_users: z.number().nullable(),
  max_workspaces: z.number().nullable(),
  max_projects: z.number().nullable(),
  max_storage_gb: z.number().nullable(),
  max_api_calls_monthly: z.number().nullable(),
});

export const PlanSchema = z.object({
  id: z.string().min(1),
  name: z.string(),
  description: z.string().nullable(),
  price_monthly: z.number(),
  price_yearly: z.number(),
  features: z.array(z.string()),
  limits: PlanLimitsSchema,
  is_public: z.boolean(),
});

export const SubscriptionSchema = z.object({
  id: z.string().min(1),
  organization_id: z.string().min(1),
  plan_id: z.string().min(1),
  plan_name: z.string(),
  status: SubscriptionStatusSchema,
  current_period_start: z.string().datetime(),
  current_period_end: z.string().datetime(),
  cancel_at_period_end: z.boolean(),
});

export const UsageSchema = z.object({
  users: z.number().min(0),
  workspaces: z.number().min(0),
  projects: z.number().min(0),
  storage_gb: z.number().min(0),
  api_calls: z.number().min(0),
  period_start: z.string().datetime(),
  period_end: z.string().datetime(),
});

export const LimitsSchema = z.object({
  max_users: z.number().nullable(),
  max_workspaces: z.number().nullable(),
  max_projects: z.number().nullable(),
  max_storage_gb: z.number().nullable(),
  max_api_calls_monthly: z.number().nullable(),
});

export const PlansResponseSchema = z.object({
  plans: z.array(PlanSchema),
});

export const PlanResponseSchema = z.object({
  plan: PlanSchema,
});

export const SubscriptionResponseSchema = z.object({
  subscription: SubscriptionSchema,
});

export const UsageResponseSchema = UsageSchema;

export const LimitsResponseSchema = LimitsSchema;

export type PlanZ = z.infer<typeof PlanSchema>;
export type SubscriptionZ = z.infer<typeof SubscriptionSchema>;
export type UsageZ = z.infer<typeof UsageSchema>;
export type LimitsZ = z.infer<typeof LimitsSchema>;
export type PlansResponse = z.infer<typeof PlansResponseSchema>;
export type PlanResponse = z.infer<typeof PlanResponseSchema>;
export type SubscriptionResponse = z.infer<typeof SubscriptionResponseSchema>;
export type UsageResponse = z.infer<typeof UsageResponseSchema>;
export type LimitsResponse = z.infer<typeof LimitsResponseSchema>;

// =============================================================================
// Audit Log Schemas
// =============================================================================

export const AuditActionSchema = z.enum([
  'create',
  'update',
  'delete',
  'login',
  'logout',
  'invite',
  'accept',
  'revoke',
]);
export const AuditResourceTypeSchema = z.enum([
  'user',
  'organization',
  'workspace',
  'project',
  'task',
  'source',
  'chat',
  'invitation',
  'member',
]);

export const AuditLogSchema = z.object({
  id: z.string().min(1),
  organization_id: z.string().min(1),
  actor_id: z.string().min(1),
  actor_email: z.string().email(),
  action: AuditActionSchema,
  resource_type: AuditResourceTypeSchema,
  resource_id: z.string().min(1),
  metadata: z.record(z.string(), z.unknown()),
  created_at: z.string().datetime(),
});

export const AuditLogFiltersSchema = z.object({
  action: AuditActionSchema.optional(),
  resource_type: AuditResourceTypeSchema.optional(),
  resource_id: z.string().optional(),
  actor_id: z.string().optional(),
  start_date: z.string().optional(),
  end_date: z.string().optional(),
  limit: z.number().min(1).max(100).optional(),
  offset: z.number().min(0).optional(),
});

export const AuditLogsResponseSchema = z.object({
  logs: z.array(AuditLogSchema),
  total: z.number().min(0),
});

export type AuditActionZ = z.infer<typeof AuditActionSchema>;
export type AuditResourceTypeZ = z.infer<typeof AuditResourceTypeSchema>;
export type AuditLogZ = z.infer<typeof AuditLogSchema>;
export type AuditLogsResponse = z.infer<typeof AuditLogsResponseSchema>;

// =============================================================================
// Knowledge Base & Context Search Schemas - re-exported from features/knowledge
// =============================================================================

export type {
  CreateKnowledgeRequestZ,
  GatherContextRequestZ,
  GatheringProgressZ,
  KnowledgeEntryZ,
  KnowledgeResponse,
  KnowledgeTypeZ,
  SearchModeZ,
  SearchOptionsZ,
  SearchResponse,
  SearchResultZ,
} from '../features/knowledge/schemas';
export {
  CreateKnowledgeRequestSchema,
  GatherContextRequestSchema,
  GatheringProgressSchema,
  GatheringStatusSchema,
  KnowledgeEntrySchema,
  KnowledgeResponseSchema,
  KnowledgeTypeSchema,
  SearchModeSchema,
  SearchOptionsSchema,
  SearchResponseSchema,
  SearchResultSchema,
} from '../features/knowledge/schemas';
