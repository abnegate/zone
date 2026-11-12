import type { z } from 'zod';
import type { ActionReceipt, Question, Waiting } from '../chats/types';
import type { TaskProgressMessageSchema } from './schemas';
import type { StageId, TaskStatus } from './status';
import type { TitleDraft } from './TitleDraft';

export type { TaskStatus } from './status';
export type RunStatus = 'pending' | 'running' | 'waiting' | 'completed' | 'failed' | 'cancelled';
export type LogLevel = 'debug' | 'info' | 'warning' | 'error';
export type PrStatus = 'pending' | 'open' | 'merged' | 'closed';

export interface Task {
  id: string;
  workspace_id: string;
  project_ids: string[];
  title: string;
  /** The title is a placeholder from the description until a generated one replaces it */
  title_pending?: boolean;
  description: string;
  acceptance_criteria: string | null;
  status: TaskStatus;
  priority: number | null;
  model_name: string | null;
  dependencies: string[];
  created_at: string | null;
  updated_at: string | null;
  started_at: string | null;
  completed_at: string | null;
  /** Whether this task uses agentic tools (file read/write, KB search, etc.) */
  is_agentic: boolean;
  /** A run submits a plan and waits for approval before changing anything */
  require_plan_approval: boolean;
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

/// The question a waiting run parked on, carried on the run itself rather than
/// on a log line: the console can be opened long after the question was asked,
/// and the reader still has to see what the run is waiting for.
export interface PendingQuestion {
  tool_call_id: string;
  questions: Question[];
}

export interface TaskRun {
  id: string;
  task_id: string;
  status: RunStatus;
  current_phase: string | null;
  progress_percent: number | null;
  error_message: string | null;
  started_at?: string | null;
  completed_at?: string | null;
  /// Present only while the run is parked, and absent on every run written
  /// before the field existed, so it is optional as well as nullable.
  pending_question?: PendingQuestion | null;
  /// The sibling of the question: a run parks on one or the other, and which
  /// arrived is how the reader tells a wait nobody can answer from a question
  /// nobody has. Sent only while the run is parked on it.
  waiting_on?: Waiting | null;
}

export interface TaskRunLog {
  id: string;
  run_id?: string;
  phase: string;
  agent_type: string;
  level: LogLevel;
  message: string;
  metadata?: { action_receipt?: ActionReceipt; [key: string]: unknown } | null;
  created_at: string;
}

export interface CreateTaskRequest {
  project_ids?: string[];
  /** Omitted, the server titles the task from its description */
  title?: string;
  description: string;
  acceptance_criteria?: string;
  priority?: number;
  model_name?: string;
  dependencies?: string[];
  /** Whether this task should use agentic tools */
  is_agentic?: boolean;
  /** Require a run's plan to be approved before it changes anything */
  require_plan_approval?: boolean;
  /** @deprecated Use source_id or source_ids instead */
  github_repo_url?: string;
  /** Single source ID for agentic tasks (overrides project source) */
  source_id?: string;
  /** Multiple source IDs for agentic tasks */
  source_ids?: string[];
}

export interface Stage {
  id: StageId;
  label: string;
  target: TaskStatus;
}

export interface Renaming {
  id: string;
  draft: TitleDraft;
}

export interface BoardActions {
  open: (task: Task, host?: Task) => void;
  run: (task: Task) => void;
  rename: (task: Task) => void;
  retitle: (task: Task, title: string) => Promise<void>;
  finish: (task: Task) => void;
  remove: (task: Task) => void;
  move: (task: Task, status: TaskStatus) => Promise<boolean>;
}

export interface UpdateTaskRequest {
  title?: string;
  description?: string;
  acceptance_criteria?: string;
  status?: TaskStatus;
  priority?: number;
  model_name?: string;
  dependencies?: string[];
  project_ids?: string[];
  /** Whether this task should use agentic tools */
  is_agentic?: boolean;
  /** Require a run's plan to be approved before it changes anything */
  require_plan_approval?: boolean;
  /** @deprecated Use source_id or source_ids instead */
  github_repo_url?: string;
  /** Single source ID for agentic tasks (overrides project source) */
  source_id?: string;
  /** Multiple source IDs for agentic tasks */
  source_ids?: string[];
}

export type TaskChanges = Pick<
  UpdateTaskRequest,
  'status' | 'priority' | 'project_ids' | 'is_agentic' | 'require_plan_approval'
>;

export interface UpdateTasksRequest extends TaskChanges {
  ids: string[];
}

export type TaskProgressMessage = z.infer<typeof TaskProgressMessageSchema>;

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

export interface AnswersResponse {
  run_id: string;
  answered: number;
}

export interface TaskRunLogsResponse {
  success?: boolean;
  error?: string;
  logs: TaskRunLog[];
}
