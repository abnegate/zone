// Task Types
export type TaskStatus = 'created' | 'queued' | 'in_progress' | 'blocked' | 'review' | 'complete';
export type RunStatus = 'running' | 'completed' | 'failed' | 'cancelled';
export type LogLevel = 'debug' | 'info' | 'warning' | 'error';
export type PrStatus = 'pending' | 'open' | 'merged' | 'closed';

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
  /** URL of the created pull request */
  pr_url: string | null;
  /** Name of the branch created for this task */
  branch_name: string | null;
  /** Status of the pull request: pending, open, merged, closed */
  pr_status: PrStatus | null;
  /** When the pull request was created */
  pr_created_at: string | null;
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
export interface TasksResponse {
  success?: boolean;
  error?: string;
  tasks: Task[];
}

export interface TaskResponse {
  success?: boolean;
  error?: string;
  task: Task;
}

export interface TaskRunsResponse {
  success?: boolean;
  error?: string;
  runs: TaskRun[];
}

export interface TaskRunResponse {
  success?: boolean;
  error?: string;
  run: TaskRun;
}

export interface TaskRunLogsResponse {
  success?: boolean;
  error?: string;
  logs: TaskRunLog[];
}
