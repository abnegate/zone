import { credentialFields, emptyCredentials, type ProviderCredentials } from '../ai/options';
import { AiProviderSchema } from './schemas';
import type {
  AiProvider,
  AiSettings,
  CommonAiSettings,
  UpdateAiSettingsRequest,
  WorkspaceAiSettings,
} from './types';

export const InheritableField = {
  Provider: 'provider',
  Fast: 'fast',
  Reasoning: 'reasoning',
  Embedding: 'embedding',
  Image: 'image',
  Video: 'video',
  Audio: 'audio',
  Runpod: 'runpod',
} as const;

export type InheritableField = (typeof InheritableField)[keyof typeof InheritableField];

export type ModelField = Exclude<InheritableField, 'provider' | 'runpod'>;

export const FieldSource = {
  Organization: 'organization',
  Workspace: 'workspace',
} as const;

export type FieldSource = (typeof FieldSource)[keyof typeof FieldSource];

type ModelKey = `model_${ModelField}`;

function isModelField(field: InheritableField): field is ModelField {
  return field !== InheritableField.Provider && field !== InheritableField.Runpod;
}

export const MODEL_FIELDS: readonly ModelField[] =
  Object.values(InheritableField).filter(isModelField);

export const FIELD_LABELS: Record<InheritableField, string> = {
  [InheritableField.Provider]: 'Provider',
  [InheritableField.Fast]: 'Fast Model',
  [InheritableField.Reasoning]: 'Reasoning Model',
  [InheritableField.Embedding]: 'Embedding Model',
  [InheritableField.Image]: 'Image Model',
  [InheritableField.Video]: 'Video Model',
  [InheritableField.Audio]: 'Audio Model',
  [InheritableField.Runpod]: 'Runpod API key',
};

export const CHAT_MODEL_FIELDS = [
  InheritableField.Fast,
  InheritableField.Reasoning,
  InheritableField.Embedding,
] as const satisfies readonly ModelField[];

const ENDPOINT_PAIRS = [
  {
    url: 'litellm_host',
    key: 'litellm_key',
    saved: 'has_litellm_key',
    urlLabel: 'LiteLLM host',
    keyLabel: 'LiteLLM API key',
  },
  {
    url: 'openai_base_url',
    key: 'openai_api_key',
    saved: 'has_openai_api_key',
    urlLabel: 'OpenAI base URL',
    keyLabel: 'OpenAI API key',
  },
  {
    url: 'anthropic_base_url',
    key: 'anthropic_api_key',
    saved: 'has_anthropic_api_key',
    urlLabel: 'Anthropic base URL',
    keyLabel: 'Anthropic API key',
  },
] as const satisfies readonly {
  url: keyof CommonAiSettings & keyof UpdateAiSettingsRequest;
  key: keyof UpdateAiSettingsRequest;
  saved: keyof CommonAiSettings;
  urlLabel: string;
  keyLabel: string;
}[];

type EndpointPair = (typeof ENDPOINT_PAIRS)[number];

const BEDROCK_LABELS = {
  region: 'AWS Bedrock region',
  credentials: 'AWS Bedrock access keys',
  iamRole: 'AWS Bedrock IAM role choice',
} as const;

const CLEARED_CHAT_MODELS: UpdateAiSettingsRequest = {
  model_fast: '',
  model_reasoning: '',
  model_embedding: '',
};

function modelKey(field: ModelField): ModelKey {
  return `model_${field}`;
}

function savesUrl(saved: CommonAiSettings, pair: EndpointPair): boolean {
  return Boolean(saved[pair.url]?.trim());
}

function savesEndpoint(saved: CommonAiSettings, pair: EndpointPair): boolean {
  return savesUrl(saved, pair) || saved[pair.saved];
}

function savesBedrock(saved: WorkspaceAiSettings): boolean {
  return (
    saved.bedrock_region !== null ||
    saved.has_bedrock_credentials ||
    saved.bedrock_use_iam_role !== null
  );
}

function withWorkspaceProvider(
  saved: WorkspaceAiSettings,
  changes: UpdateAiSettingsRequest
): UpdateAiSettingsRequest {
  const provider = workspaceProvider(saved);
  return provider === undefined ? changes : { provider, ...changes };
}

export function workspaceProvider(saved: WorkspaceAiSettings): AiProvider | undefined {
  return saved.inherits_provider ? undefined : saved.provider;
}

export function sourceOf(field: InheritableField, saved: WorkspaceAiSettings): FieldSource {
  let overridden: boolean;
  if (field === InheritableField.Provider) {
    overridden = saved.overrides && !saved.inherits_provider;
  } else if (field === InheritableField.Runpod) {
    overridden = saved.has_runpod_api_key;
  } else {
    overridden = saved[modelKey(field)] !== null;
  }
  return overridden ? FieldSource.Workspace : FieldSource.Organization;
}

export function workspaceCredentials(
  saved: WorkspaceAiSettings,
  effective: AiSettings
): ProviderCredentials {
  return {
    ...emptyCredentials,
    litellmHost: saved.litellm_host ?? '',
    openaiBaseUrl: saved.openai_base_url ?? '',
    anthropicBaseUrl: saved.anthropic_base_url ?? '',
    bedrockRegion: effective.bedrock_region || emptyCredentials.bedrockRegion,
    bedrockUseIamRole: effective.bedrock_use_iam_role,
  };
}

export function modelOverrideRequest(
  field: ModelField,
  value: string,
  saved: WorkspaceAiSettings
): UpdateAiSettingsRequest {
  const changes: UpdateAiSettingsRequest = {};
  changes[modelKey(field)] = value;
  return withWorkspaceProvider(saved, changes);
}

export function modelResetRequest(
  field: ModelField,
  saved: WorkspaceAiSettings
): UpdateAiSettingsRequest {
  return modelOverrideRequest(field, '', saved);
}

export function runpodOverrideRequest(
  key: string,
  saved: WorkspaceAiSettings
): UpdateAiSettingsRequest {
  return withWorkspaceProvider(saved, { runpod_api_key: key });
}

export function runpodResetRequest(saved: WorkspaceAiSettings): UpdateAiSettingsRequest {
  return runpodOverrideRequest('', saved);
}

function changedBedrockFields(
  credentials: ProviderCredentials,
  prefilled: ProviderCredentials
): UpdateAiSettingsRequest {
  const request: UpdateAiSettingsRequest = {};
  if (credentials.bedrockRegion !== prefilled.bedrockRegion) {
    request.bedrock_region = credentials.bedrockRegion;
  }
  if (credentials.bedrockUseIamRole !== prefilled.bedrockUseIamRole) {
    request.bedrock_use_iam_role = credentials.bedrockUseIamRole;
  }
  if (!credentials.bedrockUseIamRole) {
    if (credentials.bedrockAccessKey) request.bedrock_access_key = credentials.bedrockAccessKey;
    if (credentials.bedrockSecretKey) request.bedrock_secret_key = credentials.bedrockSecretKey;
  }
  return request;
}

export function providerOverrideRequest(
  provider: AiProvider,
  credentials: ProviderCredentials,
  prefilled: ProviderCredentials,
  saved: WorkspaceAiSettings,
  effectiveProvider: AiProvider
): UpdateAiSettingsRequest {
  const fields =
    provider === AiProviderSchema.enum.bedrock
      ? changedBedrockFields(credentials, prefilled)
      : credentialFields(provider, { ...credentials, runpodApiKey: '' }, saved);
  return {
    provider,
    ...fields,
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
  if (savesBedrock(saved)) {
    request.bedrock_region = '';
    request.bedrock_access_key = '';
    request.bedrock_secret_key = '';
    request.bedrock_use_iam_role = null;
  }
  return request;
}

export function providerResetRemovals(saved: WorkspaceAiSettings): string[] {
  return [...chatModelRemovals(saved), ...credentialRemovals(saved)];
}

function chatModelRemovals(saved: WorkspaceAiSettings): string[] {
  return CHAT_MODEL_FIELDS.flatMap((field) => {
    const model = saved[modelKey(field)];
    return model === null ? [] : [`${FIELD_LABELS[field]}: ${model}`];
  });
}

export function credentialRemovals(saved: WorkspaceAiSettings): string[] {
  const removals: string[] = [];
  for (const pair of ENDPOINT_PAIRS) {
    if (savesUrl(saved, pair)) removals.push(pair.urlLabel);
    if (saved[pair.saved]) removals.push(pair.keyLabel);
  }
  if (saved.bedrock_region !== null) removals.push(BEDROCK_LABELS.region);
  if (saved.has_bedrock_credentials) removals.push(BEDROCK_LABELS.credentials);
  if (saved.bedrock_use_iam_role !== null) removals.push(BEDROCK_LABELS.iamRole);
  return removals;
}
