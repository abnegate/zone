// Model Types
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
}

export interface HuggingFaceModel extends BrowseModel {
  install_name: string;
  author: string;
  likes: number;
  last_modified: string;
  url: string;
  pipeline_tag: string;
}

export type ModelSource = 'ollama' | 'huggingface';

// Pull Progress Types
export interface PullProgress {
  type: 'progress' | 'step' | 'complete' | 'error';
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

// API Response Types
export interface ModelsResponse {
  models: InstalledModel[];
}

export interface BrowseResponse {
  models: BrowseModel[] | HuggingFaceModel[];
  has_more: boolean;
}

export interface ModelCardResponse {
  content: string;
}
