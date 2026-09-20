import { z } from 'zod';

// =============================================================================
// Project Schemas
// =============================================================================

export const ProjectStatusSchema = z.enum(['active', 'on_hold', 'cancelled']);

export const ProjectSchema = z.object({
  id: z.string(),
  name: z.string(),
  description: z.string().nullable(),
  status: ProjectStatusSchema,
  github_repo_url: z.string().nullable(),
  source_id: z.string().nullable(),
  // Servers predating auto projects omit these; such a project is not automated.
  auto: z.boolean().default(false),
  auto_paused_reason: z.string().nullish(),
  auto_completed_at: z.string().nullish(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const AutoProjectRequestSchema = z.object({
  brief: z.string().trim().min(1, 'Describe the project first').max(8000, 'The brief is too long'),
  model_name: z.string().optional(),
});

export const AutoProjectResponseSchema = z.object({
  chat_id: z.string(),
});

export const AutomationStageSchema = z.enum([
  'idle',
  'running',
  'no_changes',
  'awaiting_checks',
  'awaiting_reviews',
  'fixing',
  'merging',
  'post_merge',
  'merged',
  'paused',
]);

export const AutomationTaskSchema = z.object({
  task_id: z.string(),
  title: z.string(),
  status: z.string(),
  is_agentic: z.boolean(),
  kind: z.string().nullable(),
  stage: AutomationStageSchema.nullable(),
  reason: z.string().nullable(),
  runs: z.number().int(),
  review_rounds: z.number().int(),
  reviewers: z.string().nullable(),
  pr_url: z.string().nullable(),
  head: z.string().nullable(),
  checks: z.string().nullable(),
  merge_sha: z.string().nullable(),
  auto_created: z.boolean(),
});

export const ProjectAutomationSchema = z.object({
  project_id: z.string(),
  auto: z.boolean(),
  actor_id: z.string().nullable(),
  paused_reason: z.string().nullable(),
  completed_at: z.string().nullable(),
  parallelism: z.number().int(),
  planner_chat_id: z.string().nullable(),
  updates_chat_id: z.string().nullable(),
  counts: z.object({
    total: z.number().int(),
    agentic: z.number().int(),
    complete: z.number().int(),
    in_flight: z.number().int(),
    paused: z.number().int(),
  }),
  tasks: z.array(AutomationTaskSchema),
});

export const CreateProjectRequestSchema = z.object({
  name: z.string().min(1, 'Name is required'),
  workspace_id: z.string().min(1, 'Workspace is required'),
  description: z.string().optional(),
  status: ProjectStatusSchema.optional(),
  github_repo_url: z.string().optional(),
  source_id: z.string().optional(),
});

export const UpdateProjectRequestSchema = z.object({
  name: z.string().min(1).optional(),
  description: z.string().optional(),
  status: ProjectStatusSchema.optional(),
  auto: z.boolean().optional(),
  github_repo_url: z.string().optional(),
  source_id: z.string().optional(),
});

export const ProjectsResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  projects: z.array(ProjectSchema),
});

export const ProjectResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  project: ProjectSchema,
});

// =============================================================================
// Sync Configuration Schemas
// =============================================================================

export const SyncProviderSchema = z.enum(['github', 'linear']);
export const SyncDirectionSchema = z.enum(['inbound', 'outbound', 'bidirectional']);

export const SyncConfigSchema = z.object({
  id: z.string().min(1),
  project_id: z.string().min(1),
  provider: SyncProviderSchema,
  direction: SyncDirectionSchema,
  external_repo_url: z.string().optional(),
  external_project_id: z.string().optional(),
  is_active: z.boolean(),
  created_at: z.string().datetime(),
  status: z.string().optional(),
  last_synced_at: z.string().nullable().optional(),
  webhook_path: z.string().optional(),
});

export const CreateSyncConfigRequestSchema = z
  .object({
    provider: SyncProviderSchema,
    direction: SyncDirectionSchema,
    external_repo_url: z.string().url('Invalid URL').optional(),
    external_project_id: z.string().min(1, 'Project ID is required for Linear').optional(),
  })
  .refine(
    (data) => {
      // GitHub requires external_repo_url
      if (data.provider === 'github' && !data.external_repo_url) {
        return false;
      }
      // Linear requires external_project_id
      if (data.provider === 'linear' && !data.external_project_id) {
        return false;
      }
      return true;
    },
    {
      message: 'GitHub requires repository URL, Linear requires project ID',
      path: ['external_repo_url'],
    }
  );

export const SyncConfigsResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  configs: z.array(SyncConfigSchema),
});

export const SyncConfigResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  config: SyncConfigSchema,
});
