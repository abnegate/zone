import { describe, expect, it } from 'bun:test';
import {
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

  it('still rejects a provider the server does not know', () => {
    expect(AiSettingsResponseSchema.safeParse({ ...settings, provider: 'gemini' }).success).toBe(
      false
    );
  });
});
