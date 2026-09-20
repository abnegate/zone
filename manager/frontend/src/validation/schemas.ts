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

// Auth Schemas - now re-exported from features/auth

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

// Organization Schemas - re-exported from features/settings/organization

export {
  CreateOrganizationRequestSchema,
  OrganizationResponseSchema,
  OrganizationSchema,
  OrganizationsResponseSchema,
  UpdateOrganizationRequestSchema,
} from '../features/settings/organization/schemas';

// Workspace Schemas

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

// Model Schemas

export {
  BrowseModelSchema,
  BrowseResponseSchema,
  InstalledModelSchema,
  ModelSourceSchema,
  ModelsResponseSchema,
  PullProgressSchema,
} from '../features/models/schemas';

// Chat Schemas

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

// Source Schemas - re-exported from features/sources

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

// Project Schemas

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

// Task Schemas - now re-exported from features/tasks

export {
  BorderRadiusSchema,
  FontFamilySchema,
  UpdateWorkspaceThemeRequestSchema,
  WorkspaceThemeResponseSchema,
  WorkspaceThemeSchema,
} from '../features/settings/workspace/schemas';
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

import type {
  WorkspaceThemeResponseSchema,
  WorkspaceThemeSchema,
} from '../features/settings/workspace/schemas';

// Workspaces Response Schemas

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

// Content Schemas
// (Content schemas re-exported from features/sources above)

// Type Exports (inferred from schemas)

export type UserZ = z.infer<typeof UserSchema>;
export type AuthResponseZ = z.infer<typeof AuthResponseSchema>;
export type LoginRequestZ = z.infer<typeof LoginRequestSchema>;
export type RegisterRequestZ = z.infer<typeof RegisterRequestSchema>;
export type JwtPayloadZ = z.infer<typeof JwtPayloadSchema>;
export type { OrganizationZ } from '../features/settings/organization/schemas';
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
export type {
  OrganizationResponse,
  OrganizationsResponse,
} from '../features/settings/organization/schemas';
export type WorkspacesResponse = z.infer<typeof WorkspacesResponseSchema>;
export type WorkspaceResponse = z.infer<typeof WorkspaceResponseSchema>;
export type WorkspaceThemeResponse = z.infer<typeof WorkspaceThemeResponseSchema>;
export type SourceVerifyResponse = z.infer<typeof SourceVerifyResponseSchema>;

// AI Settings Schemas

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
  model_image: z.string().nullable(),
  model_video: z.string().nullable(),
  model_audio: z.string().nullable(),
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
  model_image: z.string().optional(),
  model_video: z.string().optional(),
  model_audio: z.string().optional(),
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
  model_image: z.string().nullable(),
  model_video: z.string().nullable(),
  model_audio: z.string().nullable(),
});

export type AiSettingsZ = z.infer<typeof AiSettingsSchema>;
export type AiSettingsResponse = z.infer<typeof AiSettingsResponseSchema>;

// Session Schemas

export type { SessionsResponse, SessionZ } from '../features/auth/schemas';
export { SessionSchema, SessionsResponseSchema } from '../features/auth/schemas';

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

// Billing, Usage & Audit Log Schemas - re-exported from features/settings/organization

export type {
  AuditActionZ,
  AuditLogsResponse,
  AuditLogZ,
  AuditResourceTypeZ,
  LimitsResponse,
  LimitsZ,
  PlanResponse,
  PlansResponse,
  PlanZ,
  SubscriptionResponse,
  SubscriptionZ,
  UsageResponse,
  UsageZ,
} from '../features/settings/organization/schemas';
export {
  AUDIT_ACTIONS,
  AUDIT_RESOURCE_TYPES,
  AuditActionSchema,
  AuditLogFiltersSchema,
  AuditLogSchema,
  AuditLogsResponseSchema,
  AuditResourceTypeSchema,
  LimitsResponseSchema,
  LimitsSchema,
  PlanLimitsSchema,
  PlanResponseSchema,
  PlanSchema,
  PlansResponseSchema,
  SubscriptionResponseSchema,
  SubscriptionSchema,
  SubscriptionStatusSchema,
  UsageResponseSchema,
  UsageSchema,
} from '../features/settings/organization/schemas';

// Knowledge Base & Context Search Schemas - re-exported from features/knowledge

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
