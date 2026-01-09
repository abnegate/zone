// Knowledge Base Types
export type KnowledgeType = 'text' | 'url';

export interface KnowledgeEntry {
  id: string;
  workspace_id: string;
  title: string;
  type: KnowledgeType;
  content: string;
  fetched_content: string | null;
  tags: string[];
  last_refreshed_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface CreateKnowledgeRequest {
  title: string;
  type: KnowledgeType;
  content: string;
  tags?: string[];
}

export interface KnowledgeResponse {
  entries: KnowledgeEntry[];
}

// Context Search Types
export type SearchMode = 'hybrid' | 'semantic' | 'keyword';

export interface SearchResult {
  id: string;
  source_id: string;
  source_name: string;
  content: string;
  snippet: string;
  relevance_score: number;
  metadata: Record<string, unknown>;
}

export interface SearchOptions {
  query: string;
  mode?: SearchMode;
  source_ids?: string[];
  limit?: number;
}

export interface SearchResponse {
  results: SearchResult[];
  total: number;
}

export interface GatherContextRequest {
  source_ids: string[];
}

export interface GatheringProgress {
  gathering_id: string;
  status: 'pending' | 'gathering' | 'indexing' | 'complete' | 'error';
  progress: number;
  message: string;
}
