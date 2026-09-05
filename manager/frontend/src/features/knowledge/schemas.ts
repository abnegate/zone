import { z } from 'zod';

// =============================================================================
// Knowledge Base Schemas
// =============================================================================

export const KnowledgeTypeSchema = z.enum(['text', 'url']);

export const KnowledgeEntrySchema = z
  .object({
    id: z.string().min(1),
    workspace_id: z.string().min(1),
    title: z.string(),
    type: KnowledgeTypeSchema.optional(),
    content: z.string().nullable().optional(),
    fetched_content: z.string().nullable().optional(),
    category: z.string().nullable().optional(),
    tags: z.array(z.string()).optional().default([]),
    token_count: z.number().optional(),
    is_active: z.boolean().optional(),
    source_url: z.string().nullable().optional(),
    last_fetched_at: z.string().nullable().optional(),
    last_refreshed_at: z.string().nullable().optional(),
    refresh_interval_minutes: z.number().nullable().optional(),
    last_fetch_error: z.string().nullable().optional(),
    created_at: z.string().optional(),
    updated_at: z.string().optional(),
  })
  .transform((entry) => {
    const sourceUrl = entry.source_url ?? null;
    return {
      id: entry.id,
      workspace_id: entry.workspace_id,
      title: entry.title,
      type: entry.type ?? (sourceUrl ? 'url' : 'text'),
      content: entry.content ?? sourceUrl ?? '',
      fetched_content: entry.fetched_content ?? null,
      tags: entry.tags ?? [],
      last_refreshed_at: entry.last_refreshed_at ?? entry.last_fetched_at ?? null,
      created_at: entry.created_at ?? '',
      updated_at: entry.updated_at ?? '',
    };
  });

export const CreateKnowledgeRequestSchema = z.object({
  title: z.string().min(1, 'Title is required'),
  type: KnowledgeTypeSchema,
  content: z.string().min(1, 'Content is required'),
  tags: z.array(z.string()).optional(),
});

export const KnowledgeResponseSchema = z.preprocess(
  (data) => (Array.isArray(data) ? { entries: data } : data),
  z.object({
    entries: z.array(KnowledgeEntrySchema),
  })
);

export type KnowledgeTypeZ = z.infer<typeof KnowledgeTypeSchema>;
export type KnowledgeEntryZ = z.infer<typeof KnowledgeEntrySchema>;
export type CreateKnowledgeRequestZ = z.infer<typeof CreateKnowledgeRequestSchema>;
export type KnowledgeResponse = z.infer<typeof KnowledgeResponseSchema>;

// =============================================================================
// Context Search Schemas
// =============================================================================

export const SearchModeSchema = z.enum(['hybrid', 'semantic', 'keyword']);

const ApiSearchResultSchema = z.object({
  chunk_id: z.string().min(1),
  content_item_id: z.string().min(1).optional(),
  source_id: z.string().min(1),
  similarity: z.number(),
  rrf_score: z.number().optional().nullable(),
  semantic_score: z.number().optional().nullable(),
  keyword_score: z.number().optional().nullable(),
  title: z.string(),
  uri: z.string(),
  snippet: z.string(),
});

const UiSearchResultSchema = z.object({
  id: z.string().min(1),
  source_id: z.string().min(1),
  source_name: z.string(),
  content: z.string(),
  snippet: z.string(),
  relevance_score: z.number().min(0).max(1),
  metadata: z.record(z.string(), z.unknown()).optional().default({}),
});

function displayRelevance(similarity: number, semantic: number | null | undefined): number {
  if (typeof semantic === 'number') return semantic;
  if (similarity >= 0 && similarity <= 1) return similarity;
  return 0;
}

export const SearchResultSchema = z.union([
  ApiSearchResultSchema.transform((hit) => ({
    id: hit.chunk_id,
    source_id: hit.source_id,
    source_name: hit.title,
    content: hit.snippet,
    snippet: hit.snippet,
    relevance_score: displayRelevance(hit.similarity, hit.semantic_score),
    metadata: {
      type: 'file',
      path: hit.uri,
      uri: hit.uri,
      content_item_id: hit.content_item_id,
      rrf_score: hit.rrf_score ?? undefined,
      semantic_score: hit.semantic_score ?? undefined,
      keyword_score: hit.keyword_score ?? undefined,
    },
  })),
  UiSearchResultSchema,
]);

export const SearchOptionsSchema = z.object({
  query: z.string().min(1, 'Search query is required'),
  mode: SearchModeSchema.optional(),
  source_ids: z.array(z.string()).optional(),
  limit: z.number().min(1).max(100).optional(),
});

export const SearchResponseSchema = z
  .object({
    results: z.array(SearchResultSchema),
    query: z.string().optional(),
    total_results: z.number().min(0).optional(),
    total: z.number().min(0).optional(),
    search_time_ms: z.number().optional(),
  })
  .transform((response) => ({
    results: response.results,
    total: response.total ?? response.total_results ?? response.results.length,
  }));

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
