import { z } from 'zod';
import { MODEL_CAPABILITIES } from './types';

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
  completion: z.boolean().optional(),
  tools: z.boolean().optional(),
  needs_character: z.boolean().optional(),
  name: z.string(),
  size: z.number(),
  modified_at: z.string(),
  details: ModelDetailsSchema.optional(),
});

export const ModelSizeOptionSchema = z.object({
  name: z.string(),
  label: z.string(),
  size: z.number().nullable().optional(),
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
  capabilities: z.array(z.enum(MODEL_CAPABILITIES)).nullable().optional(),
  sizes: z.array(ModelSizeOptionSchema).nullable().optional(),
  details: ModelDetailsSchema.nullable().optional(),
  source: z.enum(['ollama', 'huggingface']).optional(),
});

export const ModelSourceSchema = z.enum(['ollama', 'huggingface']);

export const ModelsResponseSchema = z.object({
  models: z.array(InstalledModelSchema),
});

export const DiskUsageSchema = z.object({
  used_bytes: z.number(),
  total_bytes: z.number(),
  available_bytes: z.number(),
  percent: z.number(),
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
  digest: z.string().optional(),
  message: z.string().optional(),
  success: z.boolean().optional(),
});
