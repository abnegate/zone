import { z } from 'zod';

// =============================================================================
// Model Schemas
// =============================================================================

export const ModelDetailsSchema = z.object({
  format: z.string().nullable().optional(),
  family: z.string().nullable().optional(),
  parameter_size: z.string().nullable().optional(),
  quantization_level: z.string().nullable().optional(),
  context_length: z.number().nullable().optional(),
  license: z.string().nullable().optional(),
  ram_required_gb: z.number().nullable().optional(),
  description: z.string().nullable().optional(),
});

export const InstalledModelSchema = z.object({
  name: z.string(),
  size: z.number(),
  modified_at: z.string(),
  details: ModelDetailsSchema.optional(),
});

export const BrowseModelSchema = z.object({
  name: z.string(),
  display_name: z.string().nullable().optional(),
  size: z.number().nullable().optional(),
  digest: z.string().nullable().optional(),
  modified_at: z.string().nullable().optional(),
  description: z.string().nullable().optional(),
  author: z.string().nullable().optional(),
  url: z.string().nullable().optional(),
  downloads: z.number().nullable().optional(),
  likes: z.number().nullable().optional(),
  tags: z.array(z.string()).nullable().optional(),
  use_cases: z.array(z.string()).nullable().optional(),
  details: ModelDetailsSchema.nullable().optional(),
  source: z.enum(['ollama', 'huggingface', 'gpt4all', 'openrouter']).optional(),
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
