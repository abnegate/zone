// =============================================================================
// Model Types
// =============================================================================

export interface InstalledModel {
  name: string;
  size: number;
  modified_at: string;
  details?: ModelDetails;
}

export interface ModelDetails {
  format?: string | null;
  family?: string | null;
  parameter_size?: string | null;
  quantization_level?: string | null;
  context_length?: number | null;
  license?: string | null;
  ram_required_gb?: number | null;
  description?: string | null;
}

export interface BrowseModel {
  name: string;
  display_name?: string | null;
  size?: number | null;
  digest?: string | null;
  modified_at?: string | null;
  description?: string | null;
  author?: string | null;
  url?: string | null;
  downloads?: number | null;
  likes?: number | null;
  tags?: string[] | null;
  use_cases?: string[] | null;
  details?: ModelDetails | null;
  source?: ModelSource;
}

export type ModelSource = 'ollama' | 'huggingface' | 'gpt4all' | 'openrouter';
export type BrowseSource = ModelSource | 'all';

export const ALL_SOURCES: ModelSource[] = ['ollama', 'huggingface', 'gpt4all', 'openrouter'];

// =============================================================================
// Pull Progress Types
// =============================================================================

export interface PullProgress {
  type: 'progress' | 'step' | 'complete' | 'error' | 'authenticated';
  status?: string;
  percent?: number;
  completed?: number;
  total?: number;
  message?: string;
  success?: boolean;
}

export interface Step {
  name: string;
  message: string;
  status: 'pending' | 'success' | 'error';
}

// =============================================================================
// API Response Types
// =============================================================================

export interface ModelsResponse {
  models: InstalledModel[];
}

export interface BrowseResponse {
  models: BrowseModel[];
  next_cursor: string | null;
}

export interface ModelCardResponse {
  content: string;
}
