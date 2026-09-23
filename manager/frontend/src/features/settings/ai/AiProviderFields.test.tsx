import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import { AiProviderSchema } from '../workspace/schemas';
import type { AiSettings } from '../workspace/types';
import { AiProviderFields } from './AiProviderFields';
import {
  agentOf,
  buildAiSettingsRequest,
  emptyCredentials,
  emptyModels,
  hasOverrides,
  isAgentProvider,
  nothingConfigured,
  providerOptions,
} from './options';

const inherited: AiSettings = {
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

const everyCredential = {
  ...emptyCredentials,
  litellmHost: 'http://litellm:4000',
  litellmKey: 'sk-litellm',
  openaiApiKey: 'sk-openai',
  openaiBaseUrl: 'https://api.openai.com/v1',
  anthropicApiKey: 'sk-ant',
  anthropicBaseUrl: 'https://api.anthropic.com',
  bedrockAccessKey: 'AKIA1',
  bedrockSecretKey: 'bedrock-secret',
};

describe('coding agent providers', () => {
  it('offers Claude Code and Codex by their subscriptions', () => {
    expect(providerOptions).toContainEqual({
      value: 'claude_code',
      label: 'Claude Code (Claude subscription)',
    });
    expect(providerOptions).toContainEqual({
      value: 'codex',
      label: 'Codex (ChatGPT subscription)',
    });
  });

  it('labels every provider the schema knows, in its order', () => {
    expect(providerOptions.map((option) => option.value)).toEqual(AiProviderSchema.options);
    expect(providerOptions.every((option) => option.label.length > 0)).toBe(true);
  });

  it('maps each agent provider to the agent that serves it and every other provider to none', () => {
    expect(agentOf('claude_code')).toBe('claude');
    expect(agentOf('codex')).toBe('codex');
    expect(isAgentProvider('claude_code')).toBe(true);
    expect(isAgentProvider('codex')).toBe(true);
    for (const provider of ['self_hosted', 'openai', 'anthropic', 'bedrock'] as const) {
      expect(agentOf(provider)).toBeNull();
      expect(isAgentProvider(provider)).toBe(false);
    }
  });
});

describe('hasOverrides', () => {
  it('treats a workspace that only chooses a provider as overriding the organization', () => {
    expect(hasOverrides({ ...inherited, provider: 'codex' })).toBe(true);
    expect(hasOverrides({ ...inherited, provider: 'claude_code' })).toBe(true);
    expect(hasOverrides({ ...inherited, provider: 'openai' })).toBe(true);
  });

  it('treats an untouched self-hosted workspace as inheriting', () => {
    expect(hasOverrides(inherited)).toBe(false);
  });
});

describe('AiProviderFields', () => {
  it('renders the provider select and its credentials on one two-column grid', () => {
    const { container } = render(
      <AiProviderFields
        provider="self_hosted"
        onProviderChange={() => undefined}
        credentials={emptyCredentials}
        configured={{ ...nothingConfigured, litellm: true }}
        onChange={() => undefined}
      />
    );
    const grid = container.querySelector('.form-grid') as HTMLElement;
    expect(grid).not.toBeNull();
    expect(screen.getByLabelText('AI Provider').closest('.form-group')).toHaveClass(
      'form-group--full'
    );
    expect(screen.getByLabelText(/LiteLLM Host/).closest('.form-grid')).toBe(grid);
    expect(screen.getByLabelText(/LiteLLM API Key/).closest('.form-grid')).toBe(grid);
    expect(screen.getByText('(configured)')).toBeInTheDocument();
    expect(container.querySelectorAll('.settings-card')).toHaveLength(0);
  });

  it('reports each edit by field and hides the Bedrock keys behind the IAM toggle', () => {
    const onChange = mock();
    const { rerender } = render(
      <AiProviderFields
        provider="bedrock"
        onProviderChange={() => undefined}
        credentials={emptyCredentials}
        configured={nothingConfigured}
        onChange={onChange}
      />
    );
    fireEvent.change(screen.getByLabelText('Access Key ID'), { target: { value: 'AKIA1' } });
    expect(onChange).toHaveBeenCalledWith('bedrockAccessKey', 'AKIA1');
    fireEvent.click(screen.getByLabelText(/Use IAM Role/));
    expect(onChange).toHaveBeenCalledWith('bedrockUseIamRole', true);

    rerender(
      <AiProviderFields
        provider="bedrock"
        onProviderChange={() => undefined}
        credentials={{ ...emptyCredentials, bedrockUseIamRole: true }}
        configured={nothingConfigured}
        onChange={onChange}
      />
    );
    expect(screen.queryByLabelText('Access Key ID')).toBeNull();
    expect(screen.queryByLabelText('Secret Access Key')).toBeNull();
  });
});

describe('buildAiSettingsRequest', () => {
  it('sends only the selected provider credentials and keeps empty media models to clear them', () => {
    const request = buildAiSettingsRequest(
      'openai',
      { ...emptyCredentials, openaiApiKey: 'sk-1', litellmKey: 'ignored' },
      { ...emptyModels, fast: 'gpt-4o-mini' }
    );
    expect(request).toEqual({
      provider: 'openai',
      model_fast: 'gpt-4o-mini',
      model_reasoning: undefined,
      model_embedding: undefined,
      model_image: '',
      model_video: '',
      model_audio: '',
      openai_base_url: undefined,
      openai_api_key: 'sk-1',
    });
  });

  it.each(['claude_code', 'codex'] as const)(
    'sends no credentials for the %s provider, whose sign-in lives on the server',
    (provider) => {
      const request = buildAiSettingsRequest(provider, everyCredential, {
        ...emptyModels,
        fast: 'sonnet',
        reasoning: 'opus',
      });
      expect(request).toEqual({
        provider,
        model_fast: 'sonnet',
        model_reasoning: 'opus',
        model_embedding: undefined,
        model_image: '',
        model_video: '',
        model_audio: '',
      });
    }
  );
});
