import { describe, expect, it } from 'bun:test';
import { AgentProviderSchema } from '../features/settings/ai/schemas';
import * as workspace from '../features/settings/workspace/schemas';
import {
  AiProviderSchema,
  AiSettingsResponseSchema,
  AiSettingsSchema,
  UpdateAiSettingsRequestSchema,
} from './schemas';

const settings = {
  provider: 'self_hosted',
  has_litellm_key: false,
  litellm_host: null,
  has_openai_api_key: false,
  openai_base_url: null,
  has_anthropic_api_key: false,
  anthropic_base_url: null,
  bedrock_region: null,
  bedrock_use_iam_role: false,
  has_bedrock_credentials: false,
  model_fast: null,
  model_reasoning: null,
  model_embedding: null,
  model_image: null,
  model_video: null,
  model_audio: null,
};

describe('AI settings schemas', () => {
  it.each(['claude_code', 'codex'])('accepts the %s provider the server stores', (provider) => {
    expect(AiSettingsResponseSchema.parse({ ...settings, provider }).provider).toBe(provider);
    expect(AiSettingsSchema.parse({ ...settings, provider }).provider).toBe(provider);
    expect(UpdateAiSettingsRequestSchema.parse({ provider }).provider).toBe(provider);
  });

  it('validates against the one provider list the settings feature defines', () => {
    expect(AiProviderSchema).toBe(workspace.AiProviderSchema);
    expect(AiSettingsSchema).toBe(workspace.AiSettingsSchema);
    expect(AiSettingsResponseSchema).toBe(workspace.AiSettingsResponseSchema);
    expect(UpdateAiSettingsRequestSchema).toBe(workspace.UpdateAiSettingsRequestSchema);
    expect(
      AgentProviderSchema.options.every((agent) => AiProviderSchema.options.includes(agent))
    ).toBe(true);
  });

  it('still rejects a provider the server does not know', () => {
    expect(AiSettingsResponseSchema.safeParse({ ...settings, provider: 'gemini' }).success).toBe(
      false
    );
  });
});
