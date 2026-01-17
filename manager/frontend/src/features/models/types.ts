// =============================================================================
// Model Types
// =============================================================================

export interface InstalledModel {
  name: string;
  size: number;
  modified_at: string;
  details?: {
    description?: string;
    family?: string;
  };
}

export interface BrowseModel {
  name: string;
  size?: number | null;
  digest?: string | null;
  modified_at?: string | null;
  details?: {
    format?: string | null;
    family?: string | null;
    parameter_size?: string | null;
    quantization_level?: string | null;
  } | null;
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
