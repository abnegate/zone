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
  name: z.string(),
  size: z.number().nullable().optional(),
  digest: z.string().nullable().optional(),
  modified_at: z.string().nullable().optional(),
  details: z
    .object({
      format: z.string().nullable().optional(),
      family: z.string().nullable().optional(),
      parameter_size: z.string().nullable().optional(),
      quantization_level: z.string().nullable().optional(),
    })
    .nullable()
    .optional(),
});

export const ModelSourceSchema = z.enum(['ollama', 'huggingface', 'gpt4all', 'openrouter']);

export const ModelsResponseSchema = z.object({
  models: z.array(InstalledModelSchema),
});

export const BrowseResponseSchema = z.object({
  models: z.array(BrowseModelSchema),
  next_cursor: z.string().nullable(),
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
