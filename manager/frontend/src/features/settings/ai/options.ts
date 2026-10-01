import type { InstalledModel } from '../../models/types';
import { mergeStageOptions } from '../../models/utils/stageOptions';
import type { OrgRole } from '../organization/types';
import { AiProviderSchema } from '../workspace/schemas';
import type {
  AiProvider,
  AiSettings,
  OrganizationKeys,
  UpdateAiSettingsRequest,
} from '../workspace/types';
import { type Agent, type AgentProvider, AgentProviderSchema } from './schemas';
import type { AgentAccess } from './types';

const providerLabels: Record<AiProvider, string> = {
  self_hosted: 'Self-Hosted (Ollama via LiteLLM)',
  openai: 'OpenAI',
  anthropic: 'Anthropic',
  bedrock: 'AWS Bedrock',
  claude_code: 'Claude Code (Claude subscription)',
  codex: 'Codex (ChatGPT subscription)',
};

export const providerOptions: { value: AiProvider; label: string }[] = AiProviderSchema.options.map(
  (value) => ({ value, label: providerLabels[value] })
);

const agents: Record<AgentProvider, Agent> = {
  claude_code: 'claude',
  codex: 'codex',
};

export function isAgentProvider(provider: AiProvider): provider is AgentProvider {
  return AgentProviderSchema.safeParse(provider).success;
}

export function agentOf(provider: AiProvider): Agent | null {
  return isAgentProvider(provider) ? agents[provider] : null;
}

export function agentAccess(role: OrgRole | undefined, resolving: boolean): AgentAccess {
  if (role === 'owner' || role === 'admin') return 'manage';
  return resolving ? 'resolving' : 'view';
}

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
  claude_code: { fast: [], reasoning: [], embedding: [] },
  codex: { fast: [], reasoning: [], embedding: [] },
};

export interface ModelChoices {
  fast: string[];
  reasoning: string[];
  embedding: string[];
}

export function modelChoices(
  provider: AiProvider,
  installed: InstalledModel[],
  models: ModelSelection,
  agentModels: string[]
): ModelChoices {
  const stage = modelOptions[provider];
  const embedding = mergeStageOptions(stage.embedding, installed, models.embedding, 'embedding');
  if (isAgentProvider(provider)) {
    return {
      fast: withCurrent(agentModels, models.fast),
      reasoning: withCurrent(agentModels, models.reasoning),
      embedding,
    };
  }
  return {
    fast: mergeStageOptions(stage.fast, installed, models.fast, 'chat'),
    reasoning: mergeStageOptions(stage.reasoning, installed, models.reasoning, 'chat'),
    embedding,
  };
}

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

export type EndpointProvider = Extract<AiProvider, 'self_hosted' | 'openai' | 'anthropic'>;

interface EndpointFields {
  url: 'litellmHost' | 'openaiBaseUrl' | 'anthropicBaseUrl';
  key: 'litellmKey' | 'openaiApiKey' | 'anthropicApiKey';
  configured: keyof OrganizationKeys;
  savedUrl: 'litellm_host' | 'openai_base_url' | 'anthropic_base_url';
  savedKey: 'has_litellm_key' | 'has_openai_api_key' | 'has_anthropic_api_key';
}

const endpointFields: Record<EndpointProvider, EndpointFields> = {
  self_hosted: {
    url: 'litellmHost',
    key: 'litellmKey',
    configured: 'litellm',
    savedUrl: 'litellm_host',
    savedKey: 'has_litellm_key',
  },
  openai: {
    url: 'openaiBaseUrl',
    key: 'openaiApiKey',
    configured: 'openai',
    savedUrl: 'openai_base_url',
    savedKey: 'has_openai_api_key',
  },
  anthropic: {
    url: 'anthropicBaseUrl',
    key: 'anthropicApiKey',
    configured: 'anthropic',
    savedUrl: 'anthropic_base_url',
    savedKey: 'has_anthropic_api_key',
  },
};

export function isEndpointProvider(provider: AiProvider): provider is EndpointProvider {
  return provider in endpointFields;
}

function savesEndpoint(settings: AiSettings, provider: EndpointProvider): boolean {
  const fields = endpointFields[provider];
  return Boolean(settings[fields.savedUrl]?.trim()) || settings[fields.savedKey];
}

export type Routing = 'none' | 'pending' | 'routed';

export function routingOf(settings: AiSettings | null, provider: AiProvider): Routing {
  if (!settings || !isEndpointProvider(provider) || !savesEndpoint(settings, provider)) {
    return 'none';
  }
  return settings.completions_routed ? 'routed' : 'pending';
}

export function missesOrganizationKey(
  provider: AiProvider,
  credentials: ProviderCredentials,
  configured: ProviderConfigured,
  organizationKeys: OrganizationKeys
): boolean {
  if (!isEndpointProvider(provider)) return false;
  const fields = endpointFields[provider];
  return (
    Boolean(credentials[fields.url].trim()) &&
    !credentials[fields.key].trim() &&
    !configured[fields.configured] &&
    organizationKeys[fields.configured]
  );
}

export function needsKeyAgain(
  provider: AiProvider,
  credentials: ProviderCredentials,
  configured: ProviderConfigured,
  saved: AiSettings | null
): boolean {
  if (!isEndpointProvider(provider)) return false;
  const fields = endpointFields[provider];
  const savedUrl = saved?.[fields.savedUrl]?.trim() ?? '';
  return configured[fields.configured] && credentials[fields.url].trim() !== savedUrl;
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

function endpointUrl(entered: string, saved: string | null): string | undefined {
  if (entered.trim()) return entered;
  return saved?.trim() ? '' : undefined;
}

export function buildAiSettingsRequest(
  provider: AiProvider,
  credentials: ProviderCredentials,
  models: ModelSelection,
  saved: AiSettings | null = null
): UpdateAiSettingsRequest {
  const request: UpdateAiSettingsRequest = {
    provider,
    model_fast: models.fast,
    model_reasoning: models.reasoning,
    model_embedding: models.embedding,
    model_image: models.image,
    model_video: models.video,
    model_audio: models.audio,
  };
  if (isAgentProvider(provider)) {
    return request;
  }
  if (provider === 'self_hosted') {
    request.litellm_host = endpointUrl(credentials.litellmHost, saved?.litellm_host ?? null);
    if (credentials.litellmKey) request.litellm_key = credentials.litellmKey;
  } else if (provider === 'openai') {
    request.openai_base_url = endpointUrl(
      credentials.openaiBaseUrl,
      saved?.openai_base_url ?? null
    );
    if (credentials.openaiApiKey) request.openai_api_key = credentials.openaiApiKey;
  } else if (provider === 'anthropic') {
    request.anthropic_base_url = endpointUrl(
      credentials.anthropicBaseUrl,
      saved?.anthropic_base_url ?? null
    );
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
