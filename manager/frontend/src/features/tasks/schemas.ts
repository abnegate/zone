import { z } from 'zod';

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

export const PrStatusSchema = z.enum(['pending', 'open', 'merged', 'closed']);

export const TaskSchema = z.object({
  id: z.string(),
  workspace_id: z.string(),
  project_ids: z.array(z.string()),
  title: z.string(),
  description: z.string(),
  acceptance_criteria: z.string().nullable(),
  status: TaskStatusSchema,
  priority: z.number().nullable(),
  model_name: z.string().nullable(),
  dependencies: z.array(z.string()).optional().default([]),
  created_at: z.string().nullable(),
  updated_at: z.string().nullable(),
  started_at: z.string().nullable(),
  completed_at: z.string().nullable(),
  is_agentic: z.boolean(),
  github_repo_url: z.string().nullable(),
  source_id: z.string().nullable(),
  source_ids: z.array(z.string()).optional().default([]),
  queued_at: z.string().nullable(),
  worker_id: z.string().nullable(),
  pr_url: z.string().nullable(),
  branch_name: z.string().nullable(),
  pr_status: PrStatusSchema.nullable(),
  pr_created_at: z.string().nullable(),
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
  project_ids: z.array(z.string()).optional(),
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
  project_ids: z.array(z.string()).optional(),
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

export const RunTaskResponseSchema = z.object({
  run_id: z.string(),
});
