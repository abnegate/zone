import { describe, expect, it } from 'bun:test';
import { emptyCredentials, type ProviderCredentials } from '../ai/options';
import {
  FieldSource,
  InheritableField,
  inheritsEverything,
  type ModelField,
  modelOverrideRequest,
  modelResetRequest,
  providerOverrideRequest,
  providerResetRequest,
  sourceOf,
  workspaceProvider,
} from './inheritance';
import type { WorkspaceAiSettings } from './types';

const inherited: WorkspaceAiSettings = {
  provider: 'self_hosted',
  has_litellm_key: false,
  litellm_host: null,
  has_openai_api_key: false,
  openai_base_url: null,
  has_anthropic_api_key: false,
  anthropic_base_url: null,
  has_runpod_api_key: false,
  bedrock_region: null,
  bedrock_use_iam_role: false,
  has_bedrock_credentials: false,
  model_fast: null,
  model_reasoning: null,
  model_embedding: null,
  model_image: null,
  model_video: null,
  model_audio: null,
  completions_routed: false,
  overrides: false,
  inherits_provider: true,
  organization_keys: { litellm: false, openai: false, anthropic: false },
};

const settings = (changes: Partial<WorkspaceAiSettings> = {}): WorkspaceAiSettings => ({
  ...inherited,
  ...changes,
});

const explicitSelfHosted = settings({ overrides: true, inherits_provider: false });
const explicitOpenai = settings({ provider: 'openai', overrides: true, inherits_provider: false });
const modelOnlyOverride = settings({ overrides: true, model_fast: 'gpt-4o-mini' });

const credentials = (changes: Partial<ProviderCredentials> = {}): ProviderCredentials => ({
  ...emptyCredentials,
  ...changes,
});

const modelFields: { field: ModelField; key: keyof WorkspaceAiSettings }[] = [
  { field: InheritableField.Fast, key: 'model_fast' },
  { field: InheritableField.Reasoning, key: 'model_reasoning' },
  { field: InheritableField.Embedding, key: 'model_embedding' },
  { field: InheritableField.Image, key: 'model_image' },
  { field: InheritableField.Video, key: 'model_video' },
  { field: InheritableField.Audio, key: 'model_audio' },
];

describe('inheritance constants', () => {
  it('names every inheritable field and both sources', () => {
    expect(InheritableField).toEqual({
      Provider: 'provider',
      Fast: 'fast',
      Reasoning: 'reasoning',
      Embedding: 'embedding',
      Image: 'image',
      Video: 'video',
      Audio: 'audio',
    });
    expect(FieldSource).toEqual({ Organization: 'organization', Workspace: 'workspace' });
  });
});

describe('workspaceProvider', () => {
  it('omits the provider a workspace inherits', () => {
    expect(workspaceProvider(inherited)).toBeUndefined();
    expect(workspaceProvider(modelOnlyOverride)).toBeUndefined();
  });

  it('keeps an explicit provider, self-hosted included', () => {
    expect(workspaceProvider(explicitSelfHosted)).toBe('self_hosted');
    expect(workspaceProvider(explicitOpenai)).toBe('openai');
  });
});

describe('sourceOf', () => {
  it('reads the provider from the organization while the workspace inherits it', () => {
    expect(sourceOf(InheritableField.Provider, inherited)).toBe(FieldSource.Organization);
    expect(sourceOf(InheritableField.Provider, modelOnlyOverride)).toBe(FieldSource.Organization);
  });

  it('reads an explicit self-hosted provider from the workspace', () => {
    expect(sourceOf(InheritableField.Provider, explicitSelfHosted)).toBe(FieldSource.Workspace);
    expect(sourceOf(InheritableField.Provider, explicitOpenai)).toBe(FieldSource.Workspace);
  });

  it('reads the provider from the organization when no workspace row exists', () => {
    expect(
      sourceOf(InheritableField.Provider, settings({ overrides: false, inherits_provider: false }))
    ).toBe(FieldSource.Organization);
  });

  it('reads each model from the workspace only when the workspace sets it', () => {
    for (const { field, key } of modelFields) {
      expect(sourceOf(field, explicitOpenai)).toBe(FieldSource.Organization);
      expect(sourceOf(field, settings({ overrides: true, [key]: 'chosen-model' }))).toBe(
        FieldSource.Workspace
      );
    }
  });
});

describe('modelOverrideRequest', () => {
  it('sends only the model while the provider is inherited', () => {
    expect(modelOverrideRequest(InheritableField.Fast, 'gpt-4o', inherited)).toEqual({
      model_fast: 'gpt-4o',
    });
  });

  it('resends an explicit provider so the override survives the PUT', () => {
    expect(modelOverrideRequest(InheritableField.Reasoning, 'o1', explicitOpenai)).toEqual({
      provider: 'openai',
      model_reasoning: 'o1',
    });
    expect(modelOverrideRequest(InheritableField.Fast, 'llama3.2:3b', explicitSelfHosted)).toEqual({
      provider: 'self_hosted',
      model_fast: 'llama3.2:3b',
    });
  });

  it('targets the model key of every model field', () => {
    for (const { field, key } of modelFields) {
      expect(modelOverrideRequest(field, 'chosen-model', inherited)).toEqual({
        [key]: 'chosen-model',
      });
    }
  });
});

describe('modelResetRequest', () => {
  it('clears the model to inherit it, without a provider when inherited', () => {
    expect(
      modelResetRequest(InheritableField.Video, settings({ overrides: true, model_video: 'wan' }))
    ).toEqual({ model_video: '' });
  });

  it('resends an explicit provider alongside the cleared model', () => {
    expect(
      modelResetRequest(InheritableField.Fast, { ...explicitOpenai, model_fast: 'gpt-4o' })
    ).toEqual({ provider: 'openai', model_fast: '' });
    expect(modelResetRequest(InheritableField.Audio, explicitSelfHosted)).toEqual({
      provider: 'self_hosted',
      model_audio: '',
    });
  });

  it('targets the model key of every model field', () => {
    for (const { field, key } of modelFields) {
      expect(modelResetRequest(field, inherited)).toEqual({ [key]: '' });
    }
  });
});

describe('providerOverrideRequest', () => {
  it('keeps the inherited models when the provider matches the effective one', () => {
    expect(
      providerOverrideRequest(
        'openai',
        credentials({ openaiApiKey: 'sk-new', openaiBaseUrl: 'https://openai.example' }),
        inherited,
        'openai'
      )
    ).toEqual({
      provider: 'openai',
      openai_base_url: 'https://openai.example',
      openai_api_key: 'sk-new',
    });
  });

  it('clears the chat and embedding models when the provider differs from the effective one', () => {
    expect(
      providerOverrideRequest(
        'openai',
        credentials({ openaiApiKey: 'sk-new' }),
        inherited,
        'bedrock'
      )
    ).toEqual({
      provider: 'openai',
      openai_api_key: 'sk-new',
      model_fast: '',
      model_reasoning: '',
      model_embedding: '',
    });
  });

  it('pins the effective self-hosted provider without touching the models', () => {
    expect(
      providerOverrideRequest(
        'self_hosted',
        credentials({ litellmHost: 'http://litellm.local:4000' }),
        inherited,
        'self_hosted'
      )
    ).toEqual({ provider: 'self_hosted', litellm_host: 'http://litellm.local:4000' });
  });

  it('clears a saved endpoint URL the editor left blank', () => {
    expect(
      providerOverrideRequest(
        'anthropic',
        credentials(),
        settings({
          provider: 'anthropic',
          overrides: true,
          inherits_provider: false,
          anthropic_base_url: 'https://anthropic.example',
        }),
        'anthropic'
      )
    ).toEqual({ provider: 'anthropic', anthropic_base_url: '' });
  });

  it('sends the bedrock credentials with a provider switch', () => {
    expect(
      providerOverrideRequest(
        'bedrock',
        credentials({
          bedrockRegion: 'eu-west-1',
          bedrockAccessKey: 'AKIA',
          bedrockSecretKey: 'secret',
        }),
        inherited,
        'openai'
      )
    ).toEqual({
      provider: 'bedrock',
      bedrock_region: 'eu-west-1',
      bedrock_use_iam_role: false,
      bedrock_access_key: 'AKIA',
      bedrock_secret_key: 'secret',
      model_fast: '',
      model_reasoning: '',
      model_embedding: '',
    });
  });

  it('sends only the provider and the runpod key for an agent provider', () => {
    expect(
      providerOverrideRequest(
        'claude_code',
        credentials({ runpodApiKey: 'rp-key', openaiApiKey: 'ignored' }),
        inherited,
        'claude_code'
      )
    ).toEqual({ provider: 'claude_code', runpod_api_key: 'rp-key' });
  });

  it('never touches the media models', () => {
    const request = providerOverrideRequest('codex', credentials(), inherited, 'openai');

    expect(request.model_image).toBeUndefined();
    expect(request.model_video).toBeUndefined();
    expect(request.model_audio).toBeUndefined();
  });
});

describe('providerResetRequest', () => {
  it('clears the chat and embedding models and nothing else on a bare provider override', () => {
    const request = providerResetRequest(explicitOpenai);

    expect(request).toEqual({ model_fast: '', model_reasoning: '', model_embedding: '' });
    expect(request.provider).toBeUndefined();
  });

  it('clears only the endpoint pairs the workspace saved', () => {
    expect(
      providerResetRequest(
        settings({
          provider: 'openai',
          overrides: true,
          inherits_provider: false,
          litellm_host: 'http://litellm.local:4000',
          has_openai_api_key: true,
        })
      )
    ).toEqual({
      model_fast: '',
      model_reasoning: '',
      model_embedding: '',
      litellm_host: '',
      litellm_key: '',
      openai_base_url: '',
      openai_api_key: '',
    });
  });

  it('clears an anthropic pair saved with both its URL and its key', () => {
    expect(
      providerResetRequest(
        settings({
          provider: 'anthropic',
          overrides: true,
          inherits_provider: false,
          anthropic_base_url: 'https://anthropic.example',
          has_anthropic_api_key: true,
        })
      )
    ).toEqual({
      model_fast: '',
      model_reasoning: '',
      model_embedding: '',
      anthropic_base_url: '',
      anthropic_api_key: '',
    });
  });

  it('clears the runpod key when the workspace saved one', () => {
    expect(providerResetRequest({ ...explicitOpenai, has_runpod_api_key: true })).toEqual({
      model_fast: '',
      model_reasoning: '',
      model_embedding: '',
      runpod_api_key: '',
    });
  });

  it('clears every bedrock field when any one of them is set', () => {
    const bedrockCleared = {
      model_fast: '',
      model_reasoning: '',
      model_embedding: '',
      bedrock_region: '',
      bedrock_access_key: '',
      bedrock_secret_key: '',
      bedrock_use_iam_role: null,
    };
    const bedrock = settings({ provider: 'bedrock', overrides: true, inherits_provider: false });

    expect(providerResetRequest({ ...bedrock, bedrock_region: 'us-east-1' })).toEqual(
      bedrockCleared
    );
    expect(providerResetRequest({ ...bedrock, has_bedrock_credentials: true })).toEqual(
      bedrockCleared
    );
    expect(providerResetRequest({ ...bedrock, bedrock_use_iam_role: true })).toEqual(
      bedrockCleared
    );
  });

  it('stores a null IAM role rather than false so the organization role applies again', () => {
    const request = providerResetRequest(
      settings({ overrides: true, inherits_provider: false, bedrock_use_iam_role: true })
    );

    expect(request.bedrock_use_iam_role).toBeNull();
  });

  it('leaves bedrock alone when no bedrock field is set', () => {
    const request = providerResetRequest(explicitSelfHosted);

    expect(request.bedrock_region).toBeUndefined();
    expect(request.bedrock_access_key).toBeUndefined();
    expect(request.bedrock_secret_key).toBeUndefined();
    expect(request.bedrock_use_iam_role).toBeUndefined();
  });

  it('keeps the media model overrides', () => {
    expect(
      providerResetRequest({ ...explicitOpenai, model_image: 'sdxl', model_video: 'wan' })
    ).toEqual({ model_fast: '', model_reasoning: '', model_embedding: '' });
  });
});

describe('inheritsEverything', () => {
  it('is true for a workspace that sets nothing', () => {
    expect(inheritsEverything(inherited)).toBe(true);
  });

  it('treats a stored false IAM role as nothing set', () => {
    expect(inheritsEverything(settings({ overrides: true, bedrock_use_iam_role: false }))).toBe(
      true
    );
  });

  it('is true once a bedrock workspace has had its provider reset', () => {
    expect(inheritsEverything(settings({ provider: 'bedrock', overrides: true }))).toBe(true);
  });

  it('is false while the workspace sets its own provider', () => {
    expect(inheritsEverything(explicitSelfHosted)).toBe(false);
    expect(inheritsEverything(explicitOpenai)).toBe(false);
  });

  it('is false while any model is overridden', () => {
    for (const { key } of modelFields) {
      expect(inheritsEverything(settings({ overrides: true, [key]: 'chosen-model' }))).toBe(false);
    }
  });

  it('is false while any endpoint URL or key is saved', () => {
    for (const changes of [
      { litellm_host: 'http://litellm.local:4000' },
      { has_litellm_key: true },
      { openai_base_url: 'https://openai.example' },
      { has_openai_api_key: true },
      { anthropic_base_url: 'https://anthropic.example' },
      { has_anthropic_api_key: true },
      { has_runpod_api_key: true },
    ] satisfies Partial<WorkspaceAiSettings>[]) {
      expect(inheritsEverything(settings({ overrides: true, ...changes }))).toBe(false);
    }
  });

  it('is false while a bedrock field is left over', () => {
    expect(inheritsEverything(settings({ overrides: true, bedrock_region: 'us-east-1' }))).toBe(
      false
    );
    expect(inheritsEverything(settings({ overrides: true, has_bedrock_credentials: true }))).toBe(
      false
    );
    expect(inheritsEverything(settings({ overrides: true, bedrock_use_iam_role: true }))).toBe(
      false
    );
  });
});
