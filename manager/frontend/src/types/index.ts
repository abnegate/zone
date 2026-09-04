// Auth Types - now re-exported from features/auth
export type {
  User,
  AuthResponse,
  LoginRequest,
  RegisterRequest,
  VerifyEmailRequest,
  VerifyEmailResponse,
  ResendVerificationRequest,
  ResendVerificationResponse,
  ForgotPasswordRequest,
  ForgotPasswordResponse,
  ResetPasswordRequest,
  ResetPasswordResponse,
  JwtPayload,
  Session,
  SessionsResponse,
  OrgRole,
  WorkspaceRole,
  Invitation,
  InvitationDetails,
} from '../features/auth/types';

// Settings Types - re-exported from features/settings
export type {
  Organization,
  CreateOrganizationRequest,
  UpdateOrganizationRequest,
  OrganizationMember,
  AddOrgMemberRequest,
  UpdateOrgMemberRequest,
  OrgMembersResponse,
  CreateInvitationRequest,
  InvitationsResponse,
  Plan,
  PlanLimits,
  Subscription,
  Usage,
  Limits,
  PlansResponse,
  PlanResponse,
  SubscriptionResponse,
  UsageResponse,
  LimitsResponse,
  AuditAction,
  AuditResourceType,
  AuditLog,
  AuditLogFilters,
  AuditLogsResponse,
  ApiResponse,
  OrganizationsResponse,
  OrganizationResponse,
} from '../features/settings/organization/types';

export type {
  Workspace,
  CreateWorkspaceRequest,
  UpdateWorkspaceRequest,
  WorkspaceMember,
  AddWorkspaceMemberRequest,
  UpdateWorkspaceMemberRequest,
  WorkspaceMembersResponse,
  FontFamily,
  BorderRadius,
  WorkspaceTheme,
  UpdateWorkspaceThemeRequest,
  WorkspaceThemeResponse,
  AiProvider,
  AiSettings,
  UpdateAiSettingsRequest,
  AiSettingsResponse,
  WorkspacesResponse,
  WorkspaceResponse,
} from '../features/settings/workspace/types';

// Model Types
export type {
  InstalledModel,
  BrowseModel,
  ModelSource,
  ModelSort,
  ModelSizeFilter,
  BrowseOptions,
  PullProgress,
  Step,
  ModelsResponse,
  BrowseResponse,
  ModelCardResponse,
} from '../features/models/types';

// Chat Types
export type {
  MessageRole,
  Message,
  Chat,
  ChatWithMessages,
  CreateChatRequest,
  UpdateChatRequest,
  SendMessageRequest,
  ToolCallRecord,
  ChatSearchResult,
  ChatSearchOptions,
  ChatSearchResponse,
} from '../features/chats/types';

// Source Types - re-exported from features/sources
export type {
  SourceCategory,
  SourceType,
  GitHubConfig,
  GitLabConfig,
  FilesystemConfig,
  ICalConfig,
  IMAPConfig,
  DiscordConfig,
  SlackConfig,
  WebConfig,
  TextConfig,
  SourceConfig,
  Source,
  CreateSourceRequest,
  UpdateSourceRequest,
  SourceType_Info,
  ContentItem,
  ContentMetadata,
  FileMetadata,
  CalendarMetadata,
  MailMetadata,
  ChatMetadata,
  WebMetadata,
  TextMetadataType,
  ContentListResult,
} from '../features/sources/types';

// Source Response types are now exported from schemas
export type {
  SourcesResponse,
  SourceResponse,
  SourceTypesResponse,
  SourceVerifyResponse,
  ContentResponse,
} from '../features/sources/schemas';

// Task Types - now re-exported from features/tasks
export type {
  Task,
  TaskStatus,
  RunStatus,
  LogLevel,
  PrStatus,
  TaskRun,
  TaskRunLog,
  CreateTaskRequest,
  UpdateTaskRequest,
  TaskProgressMessage,
} from '../features/tasks/types';

// Knowledge Base Types - re-exported from features/knowledge
export type {
  KnowledgeType,
  KnowledgeEntry,
  CreateKnowledgeRequest,
  KnowledgeResponse,
  SearchMode,
  SearchResult,
  SearchOptions,
  SearchResponse,
  GatherContextRequest,
  GatheringProgress,
} from '../features/knowledge/types';
