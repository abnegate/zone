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
  created_at: string;
  updated_at: string;
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
