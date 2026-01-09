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
  created_at: z.string(),
  updated_at: z.string(),
});

export const CreateProjectRequestSchema = z.object({
  name: z.string().min(1, 'Name is required'),
  description: z.string().optional(),
  status: ProjectStatusSchema.optional(),
  github_repo_url: z.string().optional(),
  source_id: z.string().optional(),
});

export const UpdateProjectRequestSchema = z.object({
  name: z.string().min(1).optional(),
  description: z.string().optional(),
  status: ProjectStatusSchema.optional(),
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
