export interface InstalledModel {
  completion?: boolean;
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

export interface ModelSizeOption {
  name: string;
  label: string;
  size?: number | null;
}

export const MODEL_CAPABILITIES = [
  'text',
  'image_input',
  'image_generation',
  'audio',
  'audio_input',
  'audio_generation',
  'video_input',
  'video_generation',
  'tools',
  'embeddings',
  'reasoning',
] as const;

export type ModelCapability = (typeof MODEL_CAPABILITIES)[number];

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
  capabilities?: ModelCapability[] | null;
  sizes?: ModelSizeOption[] | null;
  details?: ModelDetails | null;
  source?: ModelSource;
}

export type ModelSource = 'ollama' | 'huggingface';
export type BrowseSource = ModelSource | 'all';

export type ModelSort =
  | 'relevance'
  | 'downloads_desc'
  | 'downloads_asc'
  | 'name_asc'
  | 'name_desc'
  | 'size_asc'
  | 'size_desc'
  | 'params_asc'
  | 'params_desc'
  | 'updated_desc'
  | 'updated_asc';

export type ModelSizeFilter = 'all' | 'small' | 'medium' | 'large' | 'xl';

export interface BrowseOptions {
  sort?: ModelSort;
  family?: string;
  size?: ModelSizeFilter;
}

export const ALL_SOURCES: ModelSource[] = ['ollama', 'huggingface'];

export const MODEL_SORT_OPTIONS: Array<{ value: ModelSort; label: string }> = [
  { value: 'relevance', label: 'Relevance' },
  { value: 'downloads_desc', label: 'Most downloads' },
  { value: 'downloads_asc', label: 'Fewest downloads' },
  { value: 'name_asc', label: 'Name A–Z' },
  { value: 'name_desc', label: 'Name Z–A' },
  { value: 'size_asc', label: 'Smallest size' },
  { value: 'size_desc', label: 'Largest size' },
  { value: 'params_asc', label: 'Fewest parameters' },
  { value: 'params_desc', label: 'Most parameters' },
  { value: 'updated_desc', label: 'Recently updated' },
  { value: 'updated_asc', label: 'Oldest updated' },
];

export const MODEL_FAMILY_FILTERS: Array<{ value: string; label: string }> = [
  { value: 'all', label: 'All' },
  { value: 'llama', label: 'Llama' },
  { value: 'mistral', label: 'Mistral' },
  { value: 'qwen', label: 'Qwen' },
  { value: 'phi', label: 'Phi' },
  { value: 'gemma', label: 'Gemma' },
  { value: 'deepseek', label: 'DeepSeek' },
  { value: 'mixtral', label: 'Mixtral' },
  { value: 'code', label: 'Code' },
];

export const MODEL_SIZE_FILTERS: Array<{ value: ModelSizeFilter; label: string }> = [
  { value: 'all', label: 'All sizes' },
  { value: 'small', label: '≤3B' },
  { value: 'medium', label: '7–13B' },
  { value: 'large', label: '30B+' },
  { value: 'xl', label: '70B+' },
];

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
