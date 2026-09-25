import { describe, expect, it } from 'bun:test';
import {
  AiSettingsResponseSchema,
  AiSettingsSchema,
  UpdateAiSettingsRequestSchema,
  WorkspaceMemberSchema,
  WorkspaceMembersResponseSchema,
} from './schemas';

describe('workspace AI settings schemas', () => {
  const inherited = {
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

  it.each(['claude_code', 'codex'])('accepts a %s workspace override', (provider) => {
    expect(AiSettingsSchema.parse({ ...inherited, provider }).provider).toBe(provider);
    expect(AiSettingsResponseSchema.parse({ ...inherited, provider }).provider).toBe(provider);
    expect(UpdateAiSettingsRequestSchema.parse({ provider }).provider).toBe(provider);
  });
});

describe('workspace member API schemas', () => {
  it('accepts members without email or display name', () => {
    const member = WorkspaceMemberSchema.parse({
      id: 'm1',
      workspace_id: 'ws-1',
      user_id: 'user-1',
      role: 'admin',
      is_active: true,
      invited_by: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    });

    expect(member).toMatchObject({
      id: 'm1',
      workspace_id: 'ws-1',
      email: '',
      display_name: null,
      joined_at: '2024-01-01T00:00:00Z',
    });
  });

  it('accepts a members list wrapper', () => {
    const result = WorkspaceMembersResponseSchema.parse({
      members: [
        {
          id: 'm1',
          workspace_id: 'ws-1',
          user_id: 'user-1',
          role: 'viewer',
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
        },
      ],
    });

    expect(result.members).toHaveLength(1);
  });
});
