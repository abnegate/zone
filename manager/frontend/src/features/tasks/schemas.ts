import { z } from 'zod';

export const TaskStatusSchema = z.enum([
  'created',
  'queued',
  'in_progress',
  'blocked',
  'review',
  'complete',
]);

export const RunStatusSchema = z.enum(['pending', 'running', 'completed', 'failed', 'cancelled']);

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
  progress_percent: z.number().nullable(),
  error_message: z.string().nullable(),
  started_at: z.string().nullable().optional(),
  completed_at: z.string().nullable().optional(),
});

export const TaskRunLogSchema = z
  .object({
    id: z.string(),
    run_id: z.string().optional(),
    phase: z.string(),
    agent_type: z.string(),
    log_level: LogLevelSchema.optional(),
    level: LogLevelSchema.optional(),
    message: z.string(),
    created_at: z.string(),
  })
  .transform(({ log_level, level, ...log }, context) => {
    const severity = log_level ?? level;
    if (severity === undefined) {
      context.addIssue({ code: z.ZodIssueCode.custom, message: 'Log level is required' });
      return z.NEVER;
    }
    return { ...log, level: severity };
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

export const TaskProgressMessageSchema = z.discriminatedUnion('type', [
  z.object({
    type: z.literal('init'),
    run_id: z.string(),
    task_id: z.string(),
    status: RunStatusSchema,
  }),
  z.object({
    type: z.literal('status_update'),
    status: RunStatusSchema,
    current_phase: z.string().nullable(),
    progress_percent: z.number().nullable(),
  }),
  z.object({
    type: z.literal('log'),
    id: z.string(),
    phase: z.string(),
    agent_type: z.string(),
    log_level: LogLevelSchema,
    message: z.string(),
  }),
  z.object({ type: z.literal('completed'), status: RunStatusSchema }),
  z.object({ type: z.literal('failed'), error: z.string() }),
  z.object({ type: z.literal('error'), message: z.string() }),
]);

export const RunTaskResponseSchema = TaskRunResponseSchema;
