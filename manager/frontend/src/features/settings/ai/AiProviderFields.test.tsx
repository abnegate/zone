import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import { AiProviderSchema } from '../workspace/schemas';
import type { AiProvider } from '../workspace/types';
import { AiProviderFields } from './AiProviderFields';
import {
  agentAccess,
  agentOf,
  buildAiSettingsRequest,
  emptyCredentials,
  emptyModels,
  isAgentProvider,
  nothingConfigured,
  providerOptions,
} from './options';
import type { SettingsLevel } from './types';

const everyCredential = {
  ...emptyCredentials,
  litellmHost: 'http://litellm:4000',
  litellmKey: 'sk-litellm',
  openaiApiKey: 'sk-openai',
  openaiBaseUrl: 'https://api.openai.com/v1',
  anthropicApiKey: 'sk-ant',
  anthropicBaseUrl: 'https://api.anthropic.com/v1',
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

describe('agentAccess', () => {
  it('lets owners and admins manage the organization sign-ins', () => {
    expect(agentAccess('owner', false)).toBe('manage');
    expect(agentAccess('admin', false)).toBe('manage');
  });

  it('shows only the status while the role is still resolving', () => {
    expect(agentAccess(undefined, true)).toBe('resolving');
  });

  it('fails closed for a member and for a role that never resolved', () => {
    expect(agentAccess('member', false)).toBe('view');
    expect(agentAccess(undefined, false)).toBe('view');
  });
});

describe('AiProviderFields', () => {
  it('renders the provider select and its credentials on one two-column grid', () => {
    const { container } = render(
      <AiProviderFields
        level="organization"
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
        level="organization"
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
        level="organization"
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

const routing = (level: SettingsLevel) =>
  `Chats, task runs and background work in this ${level} send completions here.`;
const workspaceKey = "A workspace host needs its own key; it never receives the organization's.";
const anthropicCompatible = "Completions go through Anthropic's OpenAI-compatible endpoint.";
const bedrockDefault = "Bedrock completions still use the server's default endpoint for now.";

function renderFields(provider: AiProvider, level: SettingsLevel = 'organization') {
  return render(
    <AiProviderFields
      level={level}
      provider={provider}
      onProviderChange={() => undefined}
      credentials={emptyCredentials}
      configured={nothingConfigured}
      onChange={() => undefined}
    />
  );
}

describe('AiProviderFields endpoint copy', () => {
  it.each([
    ['self_hosted', 'LiteLLM Host', 'http://litellm:4000'],
    ['openai', 'Base URL', 'https://api.openai.com/v1'],
    ['anthropic', 'Base URL', 'https://api.anthropic.com/v1'],
  ] as const)(
    'suggests the %s endpoint by its real address and describes it with the routing line',
    (provider, label, placeholder) => {
      renderFields(provider);
      const endpoint = screen.getByLabelText(new RegExp(label));
      expect(endpoint).toHaveAttribute('placeholder', placeholder);
      const description = document.getElementById(endpoint.getAttribute('aria-describedby') ?? '');
      expect(description?.textContent).toStartWith(routing('organization'));
    }
  );

  it('tells an organization admin where its saved endpoint sends completions without the workspace key rule', () => {
    renderFields('self_hosted');
    expect(screen.getByText(routing('organization'))).toHaveClass('form-hint');
    expect(screen.queryByText(workspaceKey)).toBeNull();
  });

  it.each(['self_hosted', 'openai', 'anthropic'] as const)(
    'warns a workspace %s endpoint that it never receives the organization key',
    (provider) => {
      renderFields(provider, 'workspace');
      expect(screen.getByText(routing('workspace'))).toHaveClass('form-hint');
      expect(screen.getByText(workspaceKey)).toHaveClass('form-hint');
      expect(screen.queryByText(routing('organization'))).toBeNull();
    }
  );

  it('notes that Anthropic runs over its OpenAI-compatible endpoint only for Anthropic', () => {
    const { unmount } = renderFields('anthropic');
    expect(screen.getByText(anthropicCompatible)).toHaveClass('form-hint');
    unmount();
    renderFields('openai');
    expect(screen.queryByText(anthropicCompatible)).toBeNull();
  });

  it.each(['organization', 'workspace'] as const)(
    'tells a %s Bedrock admin that completions keep using the server default and offers no routing line',
    (level) => {
      renderFields('bedrock', level);
      expect(screen.getByText(bedrockDefault)).toHaveClass('alert', 'alert-warning');
      expect(screen.queryByText(routing(level))).toBeNull();
      expect(screen.queryByText(workspaceKey)).toBeNull();
    }
  );

  it.each(['claude_code', 'codex'] as const)(
    'shows no endpoint copy for the %s provider, which never takes an endpoint',
    (provider) => {
      renderFields(provider, 'workspace');
      expect(screen.queryByText(routing('workspace'))).toBeNull();
      expect(screen.queryByText(workspaceKey)).toBeNull();
      expect(screen.queryByText(bedrockDefault)).toBeNull();
    }
  );
});

describe('buildAiSettingsRequest', () => {
  it('sends only the selected provider credentials and an empty model for each Automatic one', () => {
    const request = buildAiSettingsRequest(
      'openai',
      { ...emptyCredentials, openaiApiKey: 'sk-1', litellmKey: 'ignored' },
      { ...emptyModels, fast: 'gpt-4o-mini' }
    );
    expect(request).toEqual({
      provider: 'openai',
      model_fast: 'gpt-4o-mini',
      model_reasoning: '',
      model_embedding: '',
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
        model_embedding: '',
        model_image: '',
        model_video: '',
        model_audio: '',
      });
    }
  );
});
