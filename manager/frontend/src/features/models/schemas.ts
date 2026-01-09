import { z } from 'zod';

// =============================================================================
// Model Schemas
// =============================================================================

export const InstalledModelSchema = z.object({
  name: z.string(),
  size: z.number(),
  modified_at: z.string(),
  details: z
    .object({
      description: z.string().optional(),
      family: z.string().optional(),
    })
    .optional(),
});

export const BrowseModelSchema = z.object({
  id: z.string(),
  name: z.string(),
  description: z.string(),
  downloads: z.number(),
  tags: z.array(z.string()),
  install_name: z.string().nullable().optional(),
  author: z.string().nullable().optional(),
  likes: z.number().nullable().optional(),
  last_modified: z.string().nullable().optional(),
  url: z.string().nullable().optional(),
});

export const ModelSourceSchema = z.enum(['ollama', 'huggingface', 'modelscope']);

export const ModelsResponseSchema = z.object({
  models: z.array(InstalledModelSchema),
});

export const BrowseResponseSchema = z.object({
  source: ModelSourceSchema,
  models: z.array(BrowseModelSchema),
  total: z.number().nullable().optional(),
  has_more: z.boolean(),
});

export const PullProgressSchema = z.object({
  type: z.enum(['progress', 'step', 'complete', 'error', 'authenticated']),
  status: z.string().optional(),
  percent: z.number().optional(),
  completed: z.number().optional(),
  total: z.number().optional(),
  message: z.string().optional(),
  success: z.boolean().optional(),
});
