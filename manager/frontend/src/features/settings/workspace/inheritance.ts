import { credentialFields, type ProviderCredentials } from '../ai/options';
import type { AiProvider, AiSettings, UpdateAiSettingsRequest, WorkspaceAiSettings } from './types';

export const InheritableField = {
  Provider: 'provider',
  Fast: 'fast',
  Reasoning: 'reasoning',
  Embedding: 'embedding',
  Image: 'image',
  Video: 'video',
  Audio: 'audio',
} as const;

export type InheritableField = (typeof InheritableField)[keyof typeof InheritableField];

export type ModelField = Exclude<InheritableField, 'provider'>;

export const FieldSource = {
  Organization: 'organization',
  Workspace: 'workspace',
} as const;

export type FieldSource = (typeof FieldSource)[keyof typeof FieldSource];

type ModelKey = `model_${ModelField}`;

export const MODEL_FIELDS: readonly ModelField[] = Object.values(InheritableField).filter(
  (field): field is ModelField => field !== InheritableField.Provider
);

const ENDPOINT_PAIRS = [
  { url: 'litellm_host', key: 'litellm_key', saved: 'has_litellm_key' },
  { url: 'openai_base_url', key: 'openai_api_key', saved: 'has_openai_api_key' },
  { url: 'anthropic_base_url', key: 'anthropic_api_key', saved: 'has_anthropic_api_key' },
] as const satisfies readonly {
  url: keyof AiSettings & keyof UpdateAiSettingsRequest;
  key: keyof UpdateAiSettingsRequest;
  saved: keyof AiSettings;
}[];

type EndpointPair = (typeof ENDPOINT_PAIRS)[number];

const CLEARED_CHAT_MODELS: UpdateAiSettingsRequest = {
  model_fast: '',
  model_reasoning: '',
  model_embedding: '',
};

function modelKey(field: ModelField): ModelKey {
  return `model_${field}`;
}

function savesEndpoint(saved: AiSettings, pair: EndpointPair): boolean {
  return Boolean(saved[pair.url]?.trim()) || saved[pair.saved];
}

function savesBedrock(saved: AiSettings): boolean {
  return (
    saved.bedrock_region !== null || saved.has_bedrock_credentials || saved.bedrock_use_iam_role
  );
}

function modelRequest(
  field: ModelField,
  value: string,
  saved: WorkspaceAiSettings
): UpdateAiSettingsRequest {
  const request: UpdateAiSettingsRequest = {};
  const provider = workspaceProvider(saved);
  if (provider !== undefined) request.provider = provider;
  request[modelKey(field)] = value;
  return request;
}

export function workspaceProvider(saved: WorkspaceAiSettings): AiProvider | undefined {
  return saved.inherits_provider ? undefined : saved.provider;
}

export function sourceOf(field: InheritableField, saved: WorkspaceAiSettings): FieldSource {
  const overridden =
    field === InheritableField.Provider
      ? saved.overrides && !saved.inherits_provider
      : saved[modelKey(field)] !== null;
  return overridden ? FieldSource.Workspace : FieldSource.Organization;
}

export function modelOverrideRequest(
  field: ModelField,
  value: string,
  saved: WorkspaceAiSettings
): UpdateAiSettingsRequest {
  return modelRequest(field, value, saved);
}

export function modelResetRequest(
  field: ModelField,
  saved: WorkspaceAiSettings
): UpdateAiSettingsRequest {
  return modelRequest(field, '', saved);
}

export function providerOverrideRequest(
  provider: AiProvider,
  credentials: ProviderCredentials,
  saved: WorkspaceAiSettings,
  effectiveProvider: AiProvider
): UpdateAiSettingsRequest {
  return {
    provider,
    ...credentialFields(provider, credentials, saved),
    ...(provider === effectiveProvider ? {} : CLEARED_CHAT_MODELS),
  };
}

export function providerResetRequest(saved: WorkspaceAiSettings): UpdateAiSettingsRequest {
  const request: UpdateAiSettingsRequest = { ...CLEARED_CHAT_MODELS };
  for (const pair of ENDPOINT_PAIRS) {
    if (!savesEndpoint(saved, pair)) continue;
    request[pair.url] = '';
    request[pair.key] = '';
  }
  if (saved.has_runpod_api_key) request.runpod_api_key = '';
  if (savesBedrock(saved)) {
    request.bedrock_region = '';
    request.bedrock_access_key = '';
    request.bedrock_secret_key = '';
    request.bedrock_use_iam_role = null;
  }
  return request;
}

export function inheritsEverything(saved: WorkspaceAiSettings): boolean {
  return (
    saved.inherits_provider &&
    MODEL_FIELDS.every((field) => saved[modelKey(field)] === null) &&
    ENDPOINT_PAIRS.every((pair) => !savesEndpoint(saved, pair)) &&
    !saved.has_runpod_api_key &&
    !savesBedrock(saved)
  );
}
