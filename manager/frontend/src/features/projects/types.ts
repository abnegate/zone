// =============================================================================
// Project Types
// =============================================================================

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
  /** Whether the project runs itself: every agentic task run, reviewed and merged unattended. */
  auto: boolean;
  /** Why automation stopped and needs a person, when it did. */
  auto_paused_reason?: string | null;
  /** When every agentic task was merged, when it was. */
  auto_completed_at?: string | null;
  created_at: string;
  updated_at: string;
}

/** The brief an auto project's interview opens with. */
export interface AutoProjectRequest {
  brief: string;
  model_name?: string;
}

export interface AutoProjectResponse {
  chat_id: string;
}

export type AutomationStage =
  | 'idle'
  | 'running'
  | 'no_changes'
  | 'awaiting_checks'
  | 'awaiting_reviews'
  | 'fixing'
  | 'merging'
  | 'post_merge'
  | 'merged'
  | 'paused';

export interface AutomationTask {
  task_id: string;
  title: string;
  status: string;
  is_agentic: boolean;
  kind: string | null;
  stage: AutomationStage | null;
  reason: string | null;
  runs: number;
  review_rounds: number;
  reviewers: string | null;
  pr_url: string | null;
  head: string | null;
  checks: string | null;
  merge_sha: string | null;
  auto_created: boolean;
}

export interface AutomationCounts {
  total: number;
  agentic: number;
  complete: number;
  in_flight: number;
  paused: number;
}

/** What automation knows about one project right now. */
export interface ProjectAutomation {
  project_id: string;
  auto: boolean;
  actor_id: string | null;
  paused_reason: string | null;
  completed_at: string | null;
  parallelism: number;
  planner_chat_id: string | null;
  updates_chat_id: string | null;
  counts: AutomationCounts;
  tasks: AutomationTask[];
}

export interface CreateProjectRequest {
  name: string;
  workspace_id: string;
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
  /** Turn automation on or off. */
  auto?: boolean;
  /** @deprecated Use source_id instead */
  github_repo_url?: string;
  source_id?: string;
}

// =============================================================================
// Sync Configuration Types
// =============================================================================

export type SyncProvider = 'github' | 'linear';
export type SyncDirection = 'inbound' | 'outbound' | 'bidirectional';

export interface SyncConfig {
  id: string;
  project_id: string;
  provider: SyncProvider;
  direction: SyncDirection;
  external_repo_url?: string;
  external_project_id?: string;
  is_active: boolean;
  created_at: string;
  /** What the server knows about the sync: `configured` until an engine has run it */
  status?: string;
  last_synced_at?: string | null;
  /** Where the provider's webhooks are received, relative to the console's origin */
  webhook_path?: string;
}

export interface CreateSyncConfigRequest {
  provider: SyncProvider;
  direction: SyncDirection;
  external_repo_url?: string;
  external_project_id?: string;
}

// =============================================================================
// API Response Types
// =============================================================================

export interface ApiResponse {
  success?: boolean;
  error?: string;
}

export interface ProjectsResponse extends ApiResponse {
  projects: Project[];
}

export interface ProjectResponse extends ApiResponse {
  project: Project;
}

export interface SyncConfigsResponse extends ApiResponse {
  configs: SyncConfig[];
}

export interface SyncConfigResponse extends ApiResponse {
  config: SyncConfig;
}
