import type { AiProvider, AiSettings, UpdateAiSettingsRequest } from '../workspace/types';

export const providerOptions: { value: AiProvider; label: string }[] = [
  { value: 'self_hosted', label: 'Self-Hosted (Ollama via LiteLLM)' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'anthropic', label: 'Anthropic' },
  { value: 'bedrock', label: 'AWS Bedrock' },
];

export const modelOptions: Record<
  AiProvider,
  { fast: string[]; reasoning: string[]; embedding: string[] }
> = {
  self_hosted: {
    fast: ['llama3.2:3b', 'llama3.1:8b', 'qwen2.5:7b', 'mistral:7b'],
    reasoning: ['deepseek-r1:7b', 'deepseek-r1:14b', 'deepseek-r1:32b', 'llama3.1:70b'],
    embedding: ['nomic-embed-text', 'mxbai-embed-large'],
  },
  openai: {
    fast: ['gpt-4o-mini', 'gpt-4o', 'gpt-4-turbo'],
    reasoning: ['gpt-4o', 'o1', 'o1-mini'],
    embedding: ['text-embedding-3-small', 'text-embedding-3-large', 'text-embedding-ada-002'],
  },
  anthropic: {
    fast: ['claude-3-haiku-20240307', 'claude-sonnet-4-20250514'],
    reasoning: ['claude-sonnet-4-20250514', 'claude-opus-4-20250514'],
    embedding: [],
  },
  bedrock: {
    fast: [
      'anthropic.claude-3-haiku-20240307-v1:0',
      'amazon.nova-lite-v1:0',
      'amazon.nova-micro-v1:0',
    ],
    reasoning: [
      'anthropic.claude-3-5-sonnet-20241022-v2:0',
      'amazon.nova-pro-v1:0',
      'anthropic.claude-3-opus-20240229-v1:0',
    ],
    embedding: ['amazon.titan-embed-text-v2:0', 'amazon.titan-embed-text-v1'],
  },
};

export const IMAGE_MODEL_OPTIONS = ['flux1-schnell-fp8.safetensors'];
export const VIDEO_MODEL_OPTIONS = ['wan2.2_ti2v_5B_fp16.safetensors'];
export const AUDIO_MODEL_OPTIONS = ['ace_step_v1_3.5b.safetensors'];

export const awsRegions = [
  'us-east-1',
  'us-west-2',
  'eu-west-1',
  'eu-central-1',
  'ap-northeast-1',
  'ap-southeast-1',
  'ap-southeast-2',
];

export interface InstalledModelOption {
  name: string;
  details?: { format?: string | null } | null;
  ready?: boolean | null;
  required_files?: string[] | null;
}

export function comfyImageOptions(installed: InstalledModelOption[], current: string): string[] {
  const fromDisk = installed
    .filter((model) => {
      const format = model.details?.format;
      return format === 'lora' || format === 'checkpoint' || format === 'diffusion_model';
    })
    .map((model) => model.name);
  return Array.from(new Set([...IMAGE_MODEL_OPTIONS, ...fromDisk, current].filter(Boolean)));
}

export function withCurrent(options: string[], current: string): string[] {
  return Array.from(new Set([...options, current].filter(Boolean)));
}

export interface ProviderCredentials {
  litellmHost: string;
  litellmKey: string;
  openaiApiKey: string;
  openaiBaseUrl: string;
  anthropicApiKey: string;
  anthropicBaseUrl: string;
  bedrockRegion: string;
  bedrockAccessKey: string;
  bedrockSecretKey: string;
  bedrockUseIamRole: boolean;
}

export const emptyCredentials: ProviderCredentials = {
  litellmHost: '',
  litellmKey: '',
  openaiApiKey: '',
  openaiBaseUrl: '',
  anthropicApiKey: '',
  anthropicBaseUrl: '',
  bedrockRegion: 'us-east-1',
  bedrockAccessKey: '',
  bedrockSecretKey: '',
  bedrockUseIamRole: false,
};

export interface ProviderConfigured {
  litellm: boolean;
  openai: boolean;
  anthropic: boolean;
  bedrock: boolean;
}

export const nothingConfigured: ProviderConfigured = {
  litellm: false,
  openai: false,
  anthropic: false,
  bedrock: false,
};

export interface ModelSelection {
  fast: string;
  reasoning: string;
  embedding: string;
  image: string;
  video: string;
  audio: string;
}

export const emptyModels: ModelSelection = {
  fast: '',
  reasoning: '',
  embedding: '',
  image: '',
  video: '',
  audio: '',
};

export function credentialsFromSettings(settings: AiSettings): ProviderCredentials {
  return {
    ...emptyCredentials,
    litellmHost: settings.litellm_host || '',
    openaiBaseUrl: settings.openai_base_url || '',
    anthropicBaseUrl: settings.anthropic_base_url || '',
    bedrockRegion: settings.bedrock_region || 'us-east-1',
    bedrockUseIamRole: settings.bedrock_use_iam_role,
  };
}

export function configuredFromSettings(settings: AiSettings): ProviderConfigured {
  return {
    litellm: settings.has_litellm_key,
    openai: settings.has_openai_api_key,
    anthropic: settings.has_anthropic_api_key,
    bedrock: settings.has_bedrock_credentials,
  };
}

export function modelsFromSettings(settings: AiSettings): ModelSelection {
  return {
    fast: settings.model_fast || '',
    reasoning: settings.model_reasoning || '',
    embedding: settings.model_embedding || '',
    image: settings.model_image || '',
    video: settings.model_video || '',
    audio: settings.model_audio || '',
  };
}

export function hasOverrides(settings: AiSettings): boolean {
  return Boolean(
    settings.has_litellm_key ||
      settings.has_openai_api_key ||
      settings.has_anthropic_api_key ||
      settings.has_bedrock_credentials ||
      settings.model_fast ||
      settings.model_reasoning ||
      settings.model_embedding ||
      settings.model_image ||
      settings.model_video ||
      settings.model_audio ||
      settings.litellm_host ||
      settings.openai_base_url ||
      settings.anthropic_base_url ||
      settings.bedrock_region
  );
}

export function buildAiSettingsRequest(
  provider: AiProvider,
  credentials: ProviderCredentials,
  models: ModelSelection
): UpdateAiSettingsRequest {
  const request: UpdateAiSettingsRequest = {
    provider,
    model_fast: models.fast || undefined,
    model_reasoning: models.reasoning || undefined,
    model_embedding: models.embedding || undefined,
    model_image: models.image,
    model_video: models.video,
    model_audio: models.audio,
  };
  if (provider === 'self_hosted') {
    request.litellm_host = credentials.litellmHost || undefined;
    if (credentials.litellmKey) request.litellm_key = credentials.litellmKey;
  } else if (provider === 'openai') {
    request.openai_base_url = credentials.openaiBaseUrl || undefined;
    if (credentials.openaiApiKey) request.openai_api_key = credentials.openaiApiKey;
  } else if (provider === 'anthropic') {
    request.anthropic_base_url = credentials.anthropicBaseUrl || undefined;
    if (credentials.anthropicApiKey) request.anthropic_api_key = credentials.anthropicApiKey;
  } else if (provider === 'bedrock') {
    request.bedrock_region = credentials.bedrockRegion || undefined;
    request.bedrock_use_iam_role = credentials.bedrockUseIamRole;
    if (!credentials.bedrockUseIamRole) {
      if (credentials.bedrockAccessKey) request.bedrock_access_key = credentials.bedrockAccessKey;
      if (credentials.bedrockSecretKey) request.bedrock_secret_key = credentials.bedrockSecretKey;
    }
  }
  return request;
}
