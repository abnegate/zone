import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import { AiProviderSchema } from '../workspace/schemas';
import type { AiProvider, AiSettings, OrganizationKeys } from '../workspace/types';
import {
  AiProviderFields,
  AUTOMATIC_HINT,
  KEYLESS_WORKSPACE_WARNING,
  UNROUTED_NOTICE,
  VERSION_HINT,
} from './AiProviderFields';
import {
  agentAccess,
  agentOf,
  buildAiSettingsRequest,
  configuredFromSettings,
  credentialsFromSettings,
  emptyCredentials,
  emptyModels,
  isAgentProvider,
  nothingConfigured,
  type ProviderConfigured,
  type ProviderCredentials,
  providerOptions,
  routingOf,
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

function renderFields(
  provider: AiProvider,
  level: SettingsLevel = 'organization',
  saved: AiSettings | null = null
) {
  return render(
    <AiProviderFields
      level={level}
      provider={provider}
      onProviderChange={() => undefined}
      credentials={emptyCredentials}
      configured={nothingConfigured}
      onChange={() => undefined}
      saved={saved}
    />
  );
}

const savedUrls = {
  self_hosted: { litellm_host: 'http://gateway.example:4000' },
  openai: { openai_base_url: 'https://proxy.example/v1' },
  anthropic: { anthropic_base_url: 'https://gateway.example' },
} as const;

function routedFor(provider: keyof typeof savedUrls): AiSettings {
  return { ...nothingSaved, ...savedUrls[provider], provider, completions_routed: true };
}

describe('AiProviderFields endpoint copy', () => {
  it.each([
    ['self_hosted', 'LiteLLM Host', 'http://litellm:4000'],
    ['openai', 'Base URL', 'https://api.openai.com/v1'],
    ['anthropic', 'Base URL', 'https://api.anthropic.com/v1'],
  ] as const)(
    'suggests the %s endpoint by its real address and describes how it is sent',
    (provider, label, placeholder) => {
      renderFields(provider, 'organization', routedFor(provider));
      const endpoint = screen.getByLabelText(new RegExp(label));
      expect(endpoint).toHaveAttribute('placeholder', placeholder);
      const description = document.getElementById(endpoint.getAttribute('aria-describedby') ?? '');
      expect(description?.textContent).toStartWith(routing('organization'));
      expect(description?.textContent).toContain(VERSION_HINT);
      expect(description?.textContent).toContain(AUTOMATIC_HINT);
    }
  );

  it('tells an organization admin where its saved endpoint sends completions without the workspace key rule', () => {
    renderFields('self_hosted', 'organization', routedFor('self_hosted'));
    expect(screen.getByText(routing('organization'))).toHaveClass('form-hint');
    expect(screen.queryByText(workspaceKey)).toBeNull();
  });

  it.each(['self_hosted', 'openai', 'anthropic'] as const)(
    'says nothing is sent to the %s endpoint when it is not saved, or saved but not yet routed',
    (provider) => {
      for (const saved of [
        null,
        nothingSaved,
        { ...routedFor(provider), completions_routed: false },
      ]) {
        const { unmount } = renderFields(provider, 'organization', saved);
        expect(screen.queryByText(routing('organization'))).toBeNull();
        expect(screen.getByText(AUTOMATIC_HINT)).toHaveClass('form-hint');
        unmount();
      }
    }
  );

  it('says nothing is sent to an endpoint whose saved values belong to another provider', () => {
    renderFields('openai', 'organization', routedFor('self_hosted'));
    expect(screen.queryByText(routing('organization'))).toBeNull();
  });

  it.each(['self_hosted', 'openai', 'anthropic'] as const)(
    'warns a workspace %s endpoint that it never receives the organization key',
    (provider) => {
      renderFields(provider, 'workspace', routedFor(provider));
      expect(screen.getByText(routing('workspace'))).toHaveClass('form-hint');
      expect(screen.getByText(workspaceKey)).toHaveClass('form-hint');
      expect(screen.queryByText(routing('organization'))).toBeNull();
    }
  );

  it.each(['self_hosted', 'openai', 'anthropic'] as const)(
    'tells a %s admin how a bare host, a trailing slash and a path are sent',
    (provider) => {
      renderFields(provider);
      expect(screen.getByText(VERSION_HINT)).toHaveClass('form-hint');
    }
  );

  it.each(['bedrock', 'claude_code'] as const)(
    'gives the %s provider, which takes no URL, no versioning hint',
    (provider) => {
      renderFields(provider);
      expect(screen.queryByText(VERSION_HINT)).toBeNull();
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
      expect(screen.queryByText(AUTOMATIC_HINT)).toBeNull();
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

const nothingSaved: AiSettings = {
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
  completions_routed: false,
};
const unroutedHost: AiSettings = { ...nothingSaved, litellm_host: 'http://localhost:11434' };
const noOrganizationKeys: OrganizationKeys = { litellm: false, openai: false, anthropic: false };

describe('routingOf', () => {
  it('holds a row saved before routing until it is saved again', () => {
    expect(routingOf(unroutedHost, 'self_hosted')).toBe('pending');
    expect(routingOf({ ...nothingSaved, has_openai_api_key: true }, 'openai')).toBe('pending');
    expect(routingOf({ ...unroutedHost, completions_routed: true }, 'self_hosted')).toBe('routed');
  });

  it('routes nothing when the provider saved no endpoint or takes none', () => {
    expect(routingOf(null, 'self_hosted')).toBe('none');
    expect(routingOf(nothingSaved, 'self_hosted')).toBe('none');
    expect(routingOf({ ...unroutedHost, litellm_host: '  ' }, 'self_hosted')).toBe('none');
    expect(routingOf(unroutedHost, 'openai')).toBe('none');
    expect(routingOf({ ...unroutedHost, provider: 'bedrock' }, 'bedrock')).toBe('none');
    expect(routingOf(unroutedHost, 'claude_code')).toBe('none');
  });
});

describe('AiProviderFields routing notice', () => {
  function renderSaved(
    saved: AiSettings | null,
    provider: AiProvider = 'self_hosted',
    level: SettingsLevel = 'organization'
  ) {
    return render(
      <AiProviderFields
        level={level}
        provider={provider}
        onProviderChange={() => undefined}
        credentials={credentialsFromSettings(saved ?? nothingSaved)}
        configured={configuredFromSettings(saved ?? nothingSaved)}
        onChange={() => undefined}
        saved={saved}
      />
    );
  }

  it.each(['organization', 'workspace'] as const)(
    'tells the %s admin whose endpoint was saved before routing that saving routes it',
    (level) => {
      renderSaved(unroutedHost, 'self_hosted', level);
      expect(screen.getByRole('status')).toHaveTextContent(UNROUTED_NOTICE);
      expect(screen.getByText(UNROUTED_NOTICE)).toHaveClass('alert', 'alert-warning');
    }
  );

  it('says nothing once the row is routed, when nothing is saved, or for another provider', () => {
    const { unmount } = renderSaved({ ...unroutedHost, completions_routed: true });
    expect(screen.queryByText(UNROUTED_NOTICE)).toBeNull();
    unmount();
    const second = renderSaved(nothingSaved);
    expect(screen.queryByText(UNROUTED_NOTICE)).toBeNull();
    second.unmount();
    renderSaved(unroutedHost, 'openai');
    expect(screen.queryByText(UNROUTED_NOTICE)).toBeNull();
  });
});

describe('AiProviderFields keyless workspace host', () => {
  function renderWorkspace(
    credentials: ProviderCredentials,
    organizationKeys: OrganizationKeys,
    options: { configured?: ProviderConfigured; level?: SettingsLevel } = {}
  ) {
    return render(
      <AiProviderFields
        level={options.level ?? 'workspace'}
        provider="openai"
        onProviderChange={() => undefined}
        credentials={credentials}
        configured={options.configured ?? nothingConfigured}
        onChange={() => undefined}
        organizationKeys={organizationKeys}
      />
    );
  }
  const keylessHost = { ...emptyCredentials, openaiBaseUrl: 'http://gateway.example:4000' };
  const organizationOpenai = { ...noOrganizationKeys, openai: true };

  it('warns a workspace host without a key while the organization saved one', () => {
    renderWorkspace(keylessHost, organizationOpenai);
    expect(screen.getByText(KEYLESS_WORKSPACE_WARNING)).toHaveClass('alert', 'alert-warning');
  });

  it('stays quiet once the host has a key, or when the organization has none to miss', () => {
    const cases: [ProviderCredentials, OrganizationKeys, ProviderConfigured, SettingsLevel][] = [
      [
        { ...keylessHost, openaiApiKey: 'sk-workspace' },
        organizationOpenai,
        nothingConfigured,
        'workspace',
      ],
      [keylessHost, organizationOpenai, { ...nothingConfigured, openai: true }, 'workspace'],
      [keylessHost, { ...noOrganizationKeys, litellm: true }, nothingConfigured, 'workspace'],
      [emptyCredentials, organizationOpenai, nothingConfigured, 'workspace'],
      [keylessHost, organizationOpenai, nothingConfigured, 'organization'],
    ];
    for (const [credentials, organizationKeys, configured, level] of cases) {
      const { unmount } = renderWorkspace(credentials, organizationKeys, { configured, level });
      expect(screen.queryByText(KEYLESS_WORKSPACE_WARNING)).toBeNull();
      unmount();
    }
  });
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

  it.each([
    ['self_hosted', 'litellm_host', { litellm_host: 'http://gateway.example:4000' }],
    ['openai', 'openai_base_url', { openai_base_url: 'https://proxy.example/v1' }],
    ['anthropic', 'anthropic_base_url', { anthropic_base_url: 'https://gateway.example' }],
  ] as const)(
    'sends an empty %s URL to clear the one saved, and none when nothing was saved',
    (provider, field, saved) => {
      const cleared = buildAiSettingsRequest(provider, emptyCredentials, emptyModels, {
        ...nothingSaved,
        ...saved,
      });
      expect(cleared[field]).toBe('');

      for (const previous of [null, nothingSaved, { ...nothingSaved, [field]: '  ' }]) {
        const untouched = buildAiSettingsRequest(provider, emptyCredentials, emptyModels, previous);
        expect(untouched[field]).toBeUndefined();
      }
    }
  );

  it('sends the entered URL whatever was saved, and never a blank key', () => {
    const request = buildAiSettingsRequest(
      'openai',
      { ...emptyCredentials, openaiBaseUrl: 'https://new.example/v1' },
      emptyModels,
      { ...nothingSaved, openai_base_url: 'https://old.example/v1', has_openai_api_key: true }
    );
    expect(request.openai_base_url).toBe('https://new.example/v1');
    expect(request).not.toHaveProperty('openai_api_key');
  });
});
