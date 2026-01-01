// Auth Types
export interface User {
  id: string;
  email: string;
  display_name: string | null;
  is_active: boolean;
  is_admin: boolean;
  created_at: string;
  updated_at: string;
  last_login_at: string | null;
}

export interface AuthResponse {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  user: User;
  roles: string[];
  permissions: string[];
}

export interface LoginRequest {
  email: string;
  password: string;
}

export interface RegisterRequest {
  email: string;
  password: string;
  display_name?: string;
}

export interface JwtPayload {
  sub: string;
  email: string;
  roles: string[];
  permissions: string[];
  iat: number;
  exp: number;
  jti: string;
}

// Organization Types
export interface Organization {
  id: string;
  name: string;
  slug: string;
  description: string | null;
  is_active: boolean;
  created_at: string;
  updated_at: string;
}

export interface CreateOrganizationRequest {
  name: string;
  slug: string;
  description?: string;
}

export interface UpdateOrganizationRequest {
  name?: string;
  slug?: string;
  description?: string;
  is_active?: boolean;
}

// Workspace Types
export interface Workspace {
  id: string;
  organization_id: string;
  name: string;
  slug: string;
  description: string | null;
  is_active: boolean;
  created_at: string;
  updated_at: string;
}

export interface CreateWorkspaceRequest {
  name: string;
  slug: string;
  description?: string;
}

export interface UpdateWorkspaceRequest {
  name?: string;
  slug?: string;
  description?: string;
  is_active?: boolean;
}

// Permission constants matching backend
export const PERMISSIONS = {
  ORGANIZATIONS: {
    CREATE: 'organizations:create',
    READ: 'organizations:read',
    UPDATE: 'organizations:update',
    DELETE: 'organizations:delete',
  },
  WORKSPACES: {
    CREATE: 'workspaces:create',
    READ: 'workspaces:read',
    UPDATE: 'workspaces:update',
    DELETE: 'workspaces:delete',
  },
  PROJECTS: {
    CREATE: 'projects:create',
    READ: 'projects:read',
    UPDATE: 'projects:update',
    DELETE: 'projects:delete',
  },
  TASKS: {
    CREATE: 'tasks:create',
    READ: 'tasks:read',
    UPDATE: 'tasks:update',
    DELETE: 'tasks:delete',
  },
  CHATS: {
    CREATE: 'chats:create',
    READ: 'chats:read',
    UPDATE: 'chats:update',
    DELETE: 'chats:delete',
  },
  SOURCES: {
    CREATE: 'sources:create',
    READ: 'sources:read',
    UPDATE: 'sources:update',
    DELETE: 'sources:delete',
  },
  MODELS: {
    CREATE: 'models:create',
    READ: 'models:read',
    UPDATE: 'models:update',
    DELETE: 'models:delete',
  },
  WIKI: {
    CREATE: 'wiki:create',
    READ: 'wiki:read',
    UPDATE: 'wiki:update',
    DELETE: 'wiki:delete',
  },
  USERS: {
    CREATE: 'users:create',
    READ: 'users:read',
    UPDATE: 'users:update',
    DELETE: 'users:delete',
  },
} as const;

// Model Types
export interface InstalledModel {
  name: string;
  size: number;
  modified_at: string;
  details?: {
    description?: string;
    family?: string;
  };
}

export interface BrowseModel {
  id: string;
  name: string;
  description: string;
  downloads: number;
  tags: string[];
  // Optional fields provided by richer sources (HuggingFace, ModelScope)
  install_name?: string | null;
  author?: string | null;
  likes?: number | null;
  last_modified?: string | null;
  url?: string | null;
}

export type ModelSource = 'ollama' | 'huggingface' | 'modelscope';

// Pull Progress Types
export interface PullProgress {
  type: 'progress' | 'step' | 'complete' | 'error' | 'authenticated';
  status?: string;
  percent?: number;
  completed?: number;
  total?: number;
  message?: string;
  success?: boolean;
}

export interface Step {
  name: string;
  message: string;
  status: 'pending' | 'success' | 'error';
}

// API Response Types
export interface ModelsResponse {
  models: InstalledModel[];
}

export interface BrowseResponse {
  source: ModelSource;
  models: BrowseModel[];
  total?: number | null;
  has_more: boolean;
}

export interface ModelCardResponse {
  content: string;
}

// Chat Types
export type MessageRole = 'user' | 'assistant' | 'system';

export interface Message {
  id: string;
  chat_id: string;
  role: MessageRole;
  content: string;
  created_at: string;
}

export interface Chat {
  id: string;
  title: string;
  model_name: string;
  created_at: string;
  updated_at: string;
  archived: boolean;
}

export interface ChatWithMessages extends Chat {
  messages: Message[];
}

export interface CreateChatRequest {
  model_name: string;
  first_message?: string;
}

export interface SendMessageRequest {
  content: string;
}

// Source Types
export type SourceCategory = 'file' | 'calendar' | 'mail' | 'chat' | 'web' | 'text';
export type SourceType =
  | 'github'
  | 'gitlab'
  | 'filesystem' // File sources
  | 'ical' // Calendar sources
  | 'imap' // Mail sources
  | 'discord'
  | 'slack' // Chat sources (future)
  | 'web'
  | 'text'; // Simple sources

// File source configs
export interface GitHubConfig {
  owner: string;
  repo: string;
  branch?: string;
  base_path?: string;
}

export interface GitLabConfig {
  project_id: string;
  host?: string;
  branch?: string;
  base_path?: string;
}

export interface FilesystemConfig {
  base_path: string;
  allow_writes?: boolean;
}

// Calendar source configs
export interface ICalConfig {
  url: string;
  refresh_interval?: number;
}

// Mail source configs
export interface IMAPConfig {
  host: string;
  port?: number;
  username: string;
  use_ssl?: boolean;
  folder?: string;
}

// Chat source configs (future)
export interface DiscordConfig {
  server_id: string;
  channel_ids?: string[];
}

export interface SlackConfig {
  workspace_id: string;
  channel_ids?: string[];
}

// Simple source configs
export interface WebConfig {
  url: string;
  headers?: Record<string, string>;
}

export interface TextConfig {
  content: string;
  label?: string;
}

export type SourceConfig =
  | GitHubConfig
  | GitLabConfig
  | FilesystemConfig
  | ICalConfig
  | IMAPConfig
  | DiscordConfig
  | SlackConfig
  | WebConfig
  | TextConfig;

export interface Source {
  id: string;
  name: string;
  source_type: SourceType;
  category: SourceCategory;
  config: SourceConfig;
  description: string | null;
  url: string;
  is_active: boolean;
  last_verified_at: string | null;
  last_error: string | null;
  created_at: string;
  updated_at: string;
}

export interface CreateSourceRequest {
  name: string;
  source_type: SourceType;
  config: SourceConfig;
  credentials?: string;
  description?: string;
}

export interface UpdateSourceRequest {
  name?: string;
  config?: SourceConfig;
  credentials?: string;
  description?: string;
  is_active?: boolean;
}

export interface SourceType_Info {
  id: SourceType;
  name: string;
  category: SourceCategory;
  enabled: boolean;
}

// Project Types
export type ProjectStatus = 'active' | 'on_hold' | 'cancelled';

export interface Project {
  id: string;
  name: string;
  description: string | null;
  status: ProjectStatus;
  /** @deprecated Use source_id instead */
  github_repo_url: string | null;
  /** ID of the linked source */
  source_id: string | null;
  created_at: string;
  updated_at: string;
}

export interface CreateProjectRequest {
  name: string;
  description?: string;
  status?: ProjectStatus;
  /** @deprecated Use source_id instead */
  github_repo_url?: string;
  source_id?: string;
}

export interface UpdateProjectRequest {
  name?: string;
  description?: string;
  status?: ProjectStatus;
  /** @deprecated Use source_id instead */
  github_repo_url?: string;
  source_id?: string;
}

// Task Types
export type TaskStatus = 'created' | 'queued' | 'in_progress' | 'blocked' | 'review' | 'complete';
export type RunStatus = 'running' | 'completed' | 'failed' | 'cancelled';
export type LogLevel = 'debug' | 'info' | 'warning' | 'error';

export interface Task {
  id: string;
  project_id: string;
  title: string;
  description: string;
  acceptance_criteria: string | null;
  status: TaskStatus;
  priority: number;
  model_name: string | null;
  dependencies: string[];
  created_at: string;
  updated_at: string;
  started_at: string | null;
  completed_at: string | null;
  /** Whether this task uses agentic tools (file read/write, KB search, etc.) */
  is_agentic: boolean;
  /** @deprecated Use source_id or source_ids instead */
  github_repo_url: string | null;
  /** Single source ID for agentic tasks (overrides project source) */
  source_id: string | null;
  /** Multiple source IDs for agentic tasks (supports files + calendar + mail etc.) */
  source_ids: string[];
  /** When the task was added to the execution queue */
  queued_at: string | null;
  /** ID of the worker currently processing this task */
  worker_id: string | null;
}

export interface TaskRun {
  id: string;
  task_id: string;
  status: RunStatus;
  current_phase: string | null;
  progress_percent: number;
  error_message: string | null;
  started_at: string;
  completed_at: string | null;
}

export interface TaskRunLog {
  id: string;
  run_id: string;
  phase: string;
  agent_type: string;
  level: LogLevel;
  message: string;
  created_at: string;
}

export interface CreateTaskRequest {
  project_id: string;
  title: string;
  description: string;
  acceptance_criteria?: string;
  priority?: number;
  model_name?: string;
  dependencies?: string[];
  /** Whether this task should use agentic tools */
  is_agentic?: boolean;
  /** @deprecated Use source_id or source_ids instead */
  github_repo_url?: string;
  /** Single source ID for agentic tasks (overrides project source) */
  source_id?: string;
  /** Multiple source IDs for agentic tasks */
  source_ids?: string[];
}

export interface UpdateTaskRequest {
  title?: string;
  description?: string;
  acceptance_criteria?: string;
  status?: TaskStatus;
  priority?: number;
  model_name?: string;
  dependencies?: string[];
  /** Whether this task should use agentic tools */
  is_agentic?: boolean;
  /** @deprecated Use source_id or source_ids instead */
  github_repo_url?: string;
  /** Single source ID for agentic tasks (overrides project source) */
  source_id?: string;
  /** Multiple source IDs for agentic tasks */
  source_ids?: string[];
}

// Task execution progress (WebSocket messages)
export interface TaskProgressMessage {
  type: 'phase_started' | 'phase_completed' | 'log' | 'complete' | 'error';
  run_id: string;
  phase?: string;
  progress_percent?: number;
  message?: string;
  agent_type?: string;
  log_level?: string;
  success?: boolean;
  error?: string;
}

// API Response wrappers
export interface ApiResponse<T> {
  success: boolean;
  error?: string;
}

export interface ChatsResponse extends ApiResponse<Chat[]> {
  chats: Chat[];
}

export interface ChatResponse extends ApiResponse<ChatWithMessages> {
  chat: ChatWithMessages;
}

export interface MessagesResponse extends ApiResponse<Message[]> {
  messages: Message[];
}

export interface MessageResponse extends ApiResponse<Message> {
  message: Message;
}

export interface ProjectsResponse extends ApiResponse<Project[]> {
  projects: Project[];
}

export interface ProjectResponse extends ApiResponse<Project> {
  project: Project;
}

export interface SourcesResponse extends ApiResponse<Source[]> {
  sources: Source[];
}

export interface SourceResponse extends ApiResponse<Source> {
  source: Source;
}

export interface SourceTypesResponse extends ApiResponse<SourceType_Info[]> {
  types: SourceType_Info[];
}

export interface SourceVerifyResponse extends ApiResponse<boolean> {
  success: boolean;
  message: string;
  item_count?: number;
}

// Content Types (unified content from all source types)
export interface ContentItem {
  id: string;
  source_id: string;
  category: SourceCategory;
  title: string;
  content: string;
  content_type: string;
  timestamp: string | null;
  url: string | null;
  metadata: ContentMetadata;
}

export type ContentMetadata =
  | FileMetadata
  | CalendarMetadata
  | MailMetadata
  | ChatMetadata
  | WebMetadata
  | TextMetadataType;

export interface FileMetadata {
  type: 'file';
  path: string;
  size: number;
  sha: string | null;
  is_directory: boolean;
}

export interface CalendarMetadata {
  type: 'calendar';
  start_time: string;
  end_time: string;
  location: string | null;
  attendees: string[];
  recurrence: string | null;
  all_day: boolean;
}

export interface MailMetadata {
  type: 'mail';
  from: string;
  to: string[];
  cc: string[];
  subject: string;
  thread_id: string | null;
  attachments: string[];
  is_read: boolean;
}

export interface ChatMetadata {
  type: 'chat';
  channel_id: string;
  channel_name: string | null;
  author_id: string;
  author_name: string;
  thread_id: string | null;
  reactions: string[];
}

export interface WebMetadata {
  type: 'web';
  status_code: number;
  headers: Record<string, string>;
  fetched_at: string;
}

export interface TextMetadataType {
  type: 'text';
  label: string | null;
}

export interface ContentListResult {
  items: ContentItem[];
  total: number;
  has_more: boolean;
}

export interface ContentResponse extends ApiResponse<ContentItem[]> {
  items: ContentItem[];
  total: number;
  has_more: boolean;
}

// Organization API Response Types
export interface OrganizationsResponse extends ApiResponse<Organization[]> {
  organizations: Organization[];
}

export interface OrganizationResponse extends ApiResponse<Organization> {
  organization: Organization;
}

// Workspace API Response Types
export interface WorkspacesResponse extends ApiResponse<Workspace[]> {
  workspaces: Workspace[];
}

export interface WorkspaceResponse extends ApiResponse<Workspace> {
  workspace: Workspace;
}

// Workspace Theme Types
export type FontFamily = 'system' | 'inter' | 'roboto' | 'open-sans' | 'lato' | 'nunito';
export type BorderRadius = 'none' | 'small' | 'medium' | 'large';

export interface WorkspaceTheme {
  id: string;
  workspace_id: string;
  primary_color_light: string;
  secondary_color_light: string;
  primary_color_dark: string;
  secondary_color_dark: string;
  font_family: FontFamily;
  font_size_base: string;
  border_radius: BorderRadius;
  created_at: string;
  updated_at: string;
}

export interface UpdateWorkspaceThemeRequest {
  primary_color_light?: string;
  secondary_color_light?: string;
  primary_color_dark?: string;
  secondary_color_dark?: string;
  font_family?: FontFamily;
  font_size_base?: string;
  border_radius?: BorderRadius;
}

export interface WorkspaceThemeResponse extends ApiResponse<WorkspaceTheme> {
  theme: WorkspaceTheme;
}
