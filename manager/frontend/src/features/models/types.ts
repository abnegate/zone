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
  id: string;
  name: string;
  description: string;
  downloads: number;
  tags: string[];
  // Optional fields provided by richer sources (HuggingFace, ModelScope)
  install_name?: string | null;
  author?: string | null;
  likes?: number | null;
  last_modified?: string | null;
  url?: string | null;
}

export type ModelSource = 'ollama' | 'huggingface' | 'modelscope';

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
  source: ModelSource;
  models: BrowseModel[];
  total?: number | null;
  has_more: boolean;
}

export interface ModelCardResponse {
  content: string;
}
