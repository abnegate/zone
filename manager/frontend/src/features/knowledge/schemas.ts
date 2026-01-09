import { z } from 'zod';

// =============================================================================
// Knowledge Base Schemas
// =============================================================================

export const KnowledgeTypeSchema = z.enum(['text', 'url']);

export const KnowledgeEntrySchema = z.object({
  id: z.string().min(1),
  workspace_id: z.string().min(1),
  title: z.string(),
  type: KnowledgeTypeSchema,
  content: z.string(),
  fetched_content: z.string().nullable(),
  tags: z.array(z.string()),
  last_refreshed_at: z.string().nullable(),
  created_at: z.string().datetime(),
  updated_at: z.string().datetime(),
});

export const CreateKnowledgeRequestSchema = z.object({
  title: z.string().min(1, 'Title is required'),
  type: KnowledgeTypeSchema,
  content: z.string().min(1, 'Content is required'),
  tags: z.array(z.string()).optional(),
});

export const KnowledgeResponseSchema = z.object({
  entries: z.array(KnowledgeEntrySchema),
});

export type KnowledgeTypeZ = z.infer<typeof KnowledgeTypeSchema>;
export type KnowledgeEntryZ = z.infer<typeof KnowledgeEntrySchema>;
export type CreateKnowledgeRequestZ = z.infer<typeof CreateKnowledgeRequestSchema>;
export type KnowledgeResponse = z.infer<typeof KnowledgeResponseSchema>;

// =============================================================================
// Context Search Schemas
// =============================================================================

export const SearchModeSchema = z.enum(['hybrid', 'semantic', 'keyword']);

export const SearchResultSchema = z.object({
  id: z.string().min(1),
  source_id: z.string().min(1),
  source_name: z.string(),
  content: z.string(),
  snippet: z.string(),
  relevance_score: z.number().min(0).max(1),
  metadata: z.record(z.unknown()),
});

export const SearchOptionsSchema = z.object({
  query: z.string().min(1, 'Search query is required'),
  mode: SearchModeSchema.optional(),
  source_ids: z.array(z.string()).optional(),
  limit: z.number().min(1).max(100).optional(),
});

export const SearchResponseSchema = z.object({
  results: z.array(SearchResultSchema),
  total: z.number().min(0),
});

export const GatherContextRequestSchema = z.object({
  source_ids: z.array(z.string().min(1)).min(1, 'At least one source is required'),
});

export const GatherContextResponseSchema = z.object({
  gathering_id: z.string().min(1),
});

export const GatheringStatusSchema = z.enum([
  'pending',
  'gathering',
  'indexing',
  'complete',
  'error',
]);

export const GatheringProgressSchema = z.object({
  gathering_id: z.string().min(1),
  status: GatheringStatusSchema,
  progress: z.number().min(0).max(100),
  message: z.string(),
});

export type SearchModeZ = z.infer<typeof SearchModeSchema>;
export type SearchResultZ = z.infer<typeof SearchResultSchema>;
export type SearchOptionsZ = z.infer<typeof SearchOptionsSchema>;
export type SearchResponse = z.infer<typeof SearchResponseSchema>;
export type GatherContextRequestZ = z.infer<typeof GatherContextRequestSchema>;
export type GatherContextResponseZ = z.infer<typeof GatherContextResponseSchema>;
export type GatheringProgressZ = z.infer<typeof GatheringProgressSchema>;
