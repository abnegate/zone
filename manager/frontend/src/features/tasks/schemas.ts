import { z } from 'zod';
import { ActionReceiptSchema, QuestionSchema, WaitingSchema } from '../chats/schemas';

export const TaskStatusSchema = z.enum([
  'created',
  'queued',
  'in_progress',
  'blocked',
  'review',
  'complete',
]);

export const RunStatusSchema = z.enum([
  'pending',
  'running',
  'waiting',
  'completed',
  'failed',
  'cancelled',
]);

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

export const PendingQuestionSchema = z.object({
  tool_call_id: z.string(),
  questions: z.array(QuestionSchema),
});

export const TaskRunSchema = z.object({
  id: z.string(),
  task_id: z.string(),
  status: RunStatusSchema,
  current_phase: z.string().nullable(),
  progress_percent: z.number().nullable(),
  error_message: z.string().nullable(),
  /**
   * On the row, and formatted for a task by the same route, but not yet on the
   * run it sends. `schemas.contract.test.ts` names these two as what the route
   * still owes and fails the day it pays, so the allowance goes with the debt.
   */
  started_at: z.string().nullable().optional(),
  completed_at: z.string().nullable().optional(),
  /**
   * Absent on every run written before the field existed and null on every run
   * that is not parked, so it is optional as well as nullable. Forgiving on the
   * same terms as the fields it borrows from the chat: a value this client
   * cannot read costs the card, never the run it sits on — the alternative is a
   * console that cannot show a run at all because the question on it is one
   * version newer than the reader's tab.
   */
  pending_question: PendingQuestionSchema.nullish().catch(undefined),
  /**
   * The sibling of the question, on the same terms: sent only while the run is
   * parked on a wait, and an unreadable one costs the subject, never the run.
   */
  waiting_on: WaitingSchema.nullish().catch(undefined),
});

const TaskRunMetadataSchema = z
  .object({ action_receipt: ActionReceiptSchema.optional() })
  .passthrough()
  .nullish();

export const TaskRunLogSchema = z
  .object({
    id: z.string(),
    run_id: z.string().optional(),
    phase: z.string(),
    agent_type: z.string(),
    log_level: LogLevelSchema.optional(),
    level: LogLevelSchema.optional(),
    message: z.string(),
    metadata: TaskRunMetadataSchema,
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

/**
 * What the answering route replies with. The answers are handed to the waiter
 * the parked worker is blocked on and the run resumes out of band, so the reply
 * confirms the submission rather than carrying a run that has not moved yet.
 */
export const AnswersResponseSchema = z.object({
  run_id: z.string(),
  answered: z.number().int().nonnegative(),
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
    metadata: TaskRunMetadataSchema,
  }),
  z.object({ type: z.literal('completed'), status: RunStatusSchema }),
  z.object({ type: z.literal('failed'), error: z.string() }),
  z.object({ type: z.literal('error'), message: z.string() }),
]);

export const RunTaskResponseSchema = TaskRunResponseSchema;
