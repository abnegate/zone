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

export {
  CreateOrganizationRequestSchema,
  OrganizationResponseSchema,
  OrganizationSchema,
  OrganizationsResponseSchema,
  UpdateOrganizationRequestSchema,
} from '../features/settings/organization/schemas';

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
export {
  BrowseModelSchema,
  BrowseResponseSchema,
  InstalledModelSchema,
  ModelSourceSchema,
  ModelsResponseSchema,
  PullProgressSchema,
} from '../features/models/schemas';
export {
  AutomationStageSchema,
  AutomationTaskSchema,
  AutoProjectRequestSchema,
  AutoProjectResponseSchema,
  CreateProjectRequestSchema,
  CreateSyncConfigRequestSchema,
  ProjectAutomationSchema,
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
export {
  BorderRadiusSchema,
  FontFamilySchema,
  UpdateWorkspaceThemeRequestSchema,
  WorkspaceThemeResponseSchema,
  WorkspaceThemeSchema,
} from '../features/settings/workspace/schemas';
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

export type {
  InvitationDetailsZ,
  OrgRoleZ,
  SessionsResponse,
  SessionZ,
  WorkspaceRoleZ,
} from '../features/auth/schemas';
export {
  InvitationDetailsSchema,
  SessionSchema,
  SessionsResponseSchema,
} from '../features/auth/schemas';
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
export type {
  AuditActionZ,
  AuditLogsResponse,
  AuditLogZ,
  AuditResourceTypeZ,
  InvitationsResponse,
  InvitationZ,
  LimitsResponse,
  LimitsZ,
  OrganizationMemberZ,
  OrgMembersResponse,
  PlanResponse,
  PlansResponse,
  PlanZ,
  SubscriptionResponse,
  SubscriptionZ,
  UsageResponse,
  UsageZ,
} from '../features/settings/organization/schemas';
export {
  AddOrgMemberRequestSchema,
  AUDIT_ACTIONS,
  AUDIT_RESOURCE_TYPES,
  AuditActionSchema,
  AuditLogFiltersSchema,
  AuditLogSchema,
  AuditLogsResponseSchema,
  AuditResourceTypeSchema,
  CreateInvitationRequestSchema,
  InvitationSchema,
  InvitationsResponseSchema,
  LimitsResponseSchema,
  LimitsSchema,
  OrganizationMemberSchema,
  OrgMembersResponseSchema,
  PlanLimitsSchema,
  PlanResponseSchema,
  PlanSchema,
  PlansResponseSchema,
  SubscriptionResponseSchema,
  SubscriptionSchema,
  SubscriptionStatusSchema,
  UpdateOrgMemberRequestSchema,
  UsageResponseSchema,
  UsageSchema,
} from '../features/settings/organization/schemas';
export type {
  AiProviderZ,
  AiSettingsResponse,
  AiSettingsZ,
  WorkspaceMembersResponse,
  WorkspaceMemberZ,
} from '../features/settings/workspace/schemas';
export {
  AddWorkspaceMemberRequestSchema,
  AiProviderSchema,
  AiSettingsResponseSchema,
  AiSettingsSchema,
  UpdateAiSettingsRequestSchema,
  UpdateWorkspaceMemberRequestSchema,
  WorkspaceAiSettingsResponseSchema,
  WorkspaceMemberSchema,
  WorkspaceMembersResponseSchema,
} from '../features/settings/workspace/schemas';
