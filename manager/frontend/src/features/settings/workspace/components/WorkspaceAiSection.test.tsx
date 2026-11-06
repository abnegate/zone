import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import fixture from '../../../../../../../runner/zone_server/tests/fixtures/agents.json';
import { ApiError } from '../../../../api/ApiError';
import { clientModule } from '../../../../test/client';
import { chooseSelect, selectOptionValues } from '../../../../test/select';
import type { OrgRole } from '../../organization/types';
import type { AiSettings, WorkspaceAiSettings } from '../types';

const mockClient = {
  getWorkspaceAiSettings: mock(),
  updateWorkspaceAiSettings: mock(),
  resetWorkspaceAiSettings: mock(),
  getEffectiveAiSettings: mock(),
};

mock.module('../../../../api/client', () => clientModule(mockClient));

const agentsApi = {
  list: mock(),
  get: mock(),
  start: mock(),
  submitCode: mock(),
  cancel: mock(),
  signOut: mock(),
};

mock.module('../../../../api/agents', () => ({ agentsApi }));

mock.module('../../../models', () => ({
  useModels: () => ({
    models: [],
    loading: false,
    error: null,
    refresh: mock(),
    deleteModel: mock(),
  }),
}));

mock.module('react-router-dom', () => ({
  useSearchParams: () => [new URLSearchParams(), mock()],
  useLocation: () => ({ pathname: '/settings', search: '', hash: '', state: null, key: 'default' }),
  useNavigate: () => mock(),
  Link: ({ children, to, ...props }: { children: ReactNode; to: string }) => (
    <a href={to} {...props}>
      {children}
    </a>
  ),
}));

const ORGANIZATION_ID = '00000000-0000-0000-0000-000000000001';
const WORKSPACE_ID = '00000000-0000-0000-0000-000000000002';

const organization = {
  id: ORGANIZATION_ID,
  name: 'Test Org',
  slug: 'test-org',
  description: null,
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};
let organizationRole: OrgRole | undefined;
let resolvingRole = false;

mock.module('../../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentOrganization: { ...organization, role: organizationRole },
    resolvingRole,
    currentWorkspace: {
      id: WORKSPACE_ID,
      organization_id: ORGANIZATION_ID,
      name: 'Test Workspace',
      slug: 'test-workspace',
      description: null,
      is_active: true,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    },
  }),
}));

let WorkspaceAiSection: typeof import('./WorkspaceAiSection').default;

beforeAll(async () => {
  WorkspaceAiSection = (await import('./WorkspaceAiSection')).default;
});

afterAll(() => {
  mock.restore();
});

const organizationSettings: AiSettings = {
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
  model_fast: 'llama3.1:8b',
  model_reasoning: 'deepseek-r1:7b',
  model_embedding: 'nomic-embed-text',
  model_image: 'flux1-schnell-fp8.safetensors',
  model_video: 'wan2.2_ti2v_5B_fp16.safetensors',
  model_audio: 'ace_step_v1_3.5b.safetensors',
  completions_routed: true,
};

const noOrganizationKeys = { litellm: false, openai: false, anthropic: false };

const inherited: WorkspaceAiSettings = {
  ...organizationSettings,
  model_fast: null,
  model_reasoning: null,
  model_embedding: null,
  model_image: null,
  model_video: null,
  model_audio: null,
  bedrock_use_iam_role: null,
  overrides: false,
  inherits_provider: true,
  organization_keys: noOrganizationKeys,
};

const overridden: WorkspaceAiSettings = {
  ...organizationSettings,
  bedrock_use_iam_role: null,
  overrides: true,
  inherits_provider: false,
  organization_keys: noOrganizationKeys,
};

const providerOnly: WorkspaceAiSettings = {
  ...inherited,
  overrides: true,
  inherits_provider: false,
};

const codexOnly: WorkspaceAiSettings = {
  ...inherited,
  provider: 'codex',
  overrides: true,
  inherits_provider: false,
};

const codexEffective: AiSettings = {
  ...organizationSettings,
  provider: 'codex',
  model_fast: null,
  model_reasoning: null,
};

const row = (label: string) => screen.getByRole('group', { name: label });

const RESET_ALL = 'Reset all AI settings to the organization';

const sentRequest = async () => {
  await waitFor(() => expect(mockClient.updateWorkspaceAiSettings).toHaveBeenCalled());
  const [organizationId, workspaceId, request] = mockClient.updateWorkspaceAiSettings.mock.calls[0];
  expect(organizationId).toBe(ORGANIZATION_ID);
  expect(workspaceId).toBe(WORKSPACE_ID);
  return request;
};

async function renderSection(
  props: { editable?: boolean; organizationDefaultsPath?: string } = {}
): Promise<ReturnType<typeof userEvent.setup>> {
  const user = userEvent.setup();
  render(
    <WorkspaceAiSection
      editable={props.editable ?? true}
      organizationDefaultsPath={props.organizationDefaultsPath}
    />
  );
  await screen.findByRole('group', { name: 'Provider' });
  return user;
}

describe('WorkspaceAiSection', () => {
  beforeEach(() => {
    mock.clearAllMocks();
    organizationRole = undefined;
    resolvingRole = false;
    agentsApi.list.mockResolvedValue(fixture.agents);
    mockClient.getWorkspaceAiSettings.mockResolvedValue(overridden);
    mockClient.getEffectiveAiSettings.mockResolvedValue(organizationSettings);
    mockClient.updateWorkspaceAiSettings.mockResolvedValue(overridden);
    mockClient.resetWorkspaceAiSettings.mockResolvedValue(inherited);
  });

  it('heads the section and says each field follows the organization', async () => {
    await renderSection();

    expect(screen.getByRole('heading', { name: 'AI Provider Settings', level: 2 })).toHaveClass(
      'section-title'
    );
    expect(
      screen.getByText('Each field follows the organization unless this workspace overrides it.')
    ).toBeInTheDocument();
    expect(mockClient.getWorkspaceAiSettings).toHaveBeenCalledWith(ORGANIZATION_ID, WORKSPACE_ID);
    expect(mockClient.getEffectiveAiSettings).toHaveBeenCalledWith(ORGANIZATION_ID, WORKSPACE_ID);
  });

  it('shows each effective value with where it comes from', async () => {
    mockClient.getWorkspaceAiSettings.mockResolvedValue({
      ...inherited,
      overrides: true,
      model_fast: 'llama3.2:3b',
    });
    mockClient.getEffectiveAiSettings.mockResolvedValue({
      ...organizationSettings,
      model_fast: 'llama3.2:3b',
      model_reasoning: null,
      model_image: null,
    });
    await renderSection();

    expect(
      within(row('Provider')).getByText('Self-Hosted (Ollama via LiteLLM)')
    ).toBeInTheDocument();
    expect(within(row('Provider')).getByText('Inherited from organization')).toBeInTheDocument();
    expect(within(row('Fast Model')).getByText('llama3.2:3b')).toBeInTheDocument();
    expect(within(row('Fast Model')).getByText('Set for this workspace')).toBeInTheDocument();
    expect(within(row('Reasoning Model')).getByText('Automatic')).toBeInTheDocument();
    expect(within(row('Image Model')).getByText('Server default')).toBeInTheDocument();
    expect(within(row('Video Model')).getByText('Inherited from organization')).toBeInTheDocument();
  });

  it('leaves the Runpod key out of the provider editor', async () => {
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Edit Provider' }));

    expect(screen.getByLabelText('AI Provider')).toBeInTheDocument();
    expect(within(row('Provider')).queryByLabelText(/^Runpod/)).toBeNull();
  });

  describe('Runpod API key', () => {
    it('says whether a key applies without showing it', async () => {
      await renderSection();

      expect(within(row('Runpod API key')).getByText('Not set')).toBeInTheDocument();
      expect(
        within(row('Runpod API key')).getByText('Inherited from organization')
      ).toBeInTheDocument();
      expect(within(row('Runpod API key')).queryByRole('textbox')).toBeNull();
    });

    it('reads a key the workspace saved as set for this workspace', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue({
        ...overridden,
        has_runpod_api_key: true,
      });
      mockClient.getEffectiveAiSettings.mockResolvedValue({
        ...organizationSettings,
        has_runpod_api_key: true,
      });
      await renderSection();

      expect(within(row('Runpod API key')).getByText('Set')).toBeInTheDocument();
      expect(within(row('Runpod API key')).getByText('Set for this workspace')).toBeInTheDocument();
    });

    it('saves a key alone while the provider is inherited', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue(inherited);
      mockClient.updateWorkspaceAiSettings.mockResolvedValue({
        ...inherited,
        overrides: true,
        has_runpod_api_key: true,
      });
      const user = await renderSection();

      await user.click(screen.getByRole('button', { name: 'Override Runpod API key' }));
      const save = screen.getByRole('button', { name: 'Save Runpod API key' });
      expect(save).toBeDisabled();
      await user.type(screen.getByLabelText('Runpod API key', { selector: 'input' }), 'rp-secret');
      await user.click(save);

      expect(await sentRequest()).toEqual({ runpod_api_key: 'rp-secret' });
      expect(await screen.findByText('Runpod API key saved')).toHaveClass('alert-success');
      expect(within(row('Provider')).getByText('Inherited from organization')).toBeInTheDocument();
    });

    it('resends the workspace provider with the key', async () => {
      const user = await renderSection();

      await user.click(screen.getByRole('button', { name: 'Override Runpod API key' }));
      await user.type(screen.getByLabelText('Runpod API key', { selector: 'input' }), 'rp-secret');
      await user.click(screen.getByRole('button', { name: 'Save Runpod API key' }));

      expect(await sentRequest()).toEqual({
        provider: 'self_hosted',
        runpod_api_key: 'rp-secret',
      });
    });

    it('asks before a reset removes the key, then clears it alone', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue({
        ...overridden,
        has_runpod_api_key: true,
      });
      const user = await renderSection();

      await user.click(screen.getByRole('button', { name: 'Reset Runpod API key' }));
      const dialog = screen.getByRole('dialog', {
        name: 'Reset Runpod API key to the organization?',
      });
      expect(within(dialog).getByText('Runpod API key')).toBeInTheDocument();
      expect(mockClient.updateWorkspaceAiSettings).not.toHaveBeenCalled();
      await user.click(within(dialog).getByRole('button', { name: 'Reset' }));

      expect(await sentRequest()).toEqual({ provider: 'self_hosted', runpod_api_key: '' });
    });

    it('keeps the key when the provider is reset', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue({
        ...providerOnly,
        has_runpod_api_key: true,
      });
      const user = await renderSection();

      await user.click(screen.getByRole('button', { name: 'Reset Provider' }));

      expect(await sentRequest()).not.toHaveProperty('runpod_api_key');
    });
  });

  it('closes the editor on Cancel without saving', async () => {
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Edit Provider' }));
    await user.click(within(row('Provider')).getByRole('button', { name: 'Cancel' }));

    expect(screen.queryByLabelText('AI Provider')).toBeNull();
    expect(mockClient.updateWorkspaceAiSettings).not.toHaveBeenCalled();
  });

  it('clears the chat models when the override changes the provider', async () => {
    mockClient.updateWorkspaceAiSettings.mockResolvedValue(codexOnly);
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Edit Provider' }));
    chooseSelect('AI Provider', 'Codex (ChatGPT subscription)');
    await user.click(screen.getByRole('button', { name: 'Save Provider' }));

    expect(await sentRequest()).toEqual({
      provider: 'codex',
      model_fast: '',
      model_reasoning: '',
      model_embedding: '',
    });
  });

  it("keeps the organization's models when the override keeps the provider", async () => {
    mockClient.getWorkspaceAiSettings.mockResolvedValue(inherited);
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Override Provider' }));
    await user.click(screen.getByRole('button', { name: 'Save Provider' }));

    expect(await sentRequest()).toEqual({ provider: 'self_hosted' });
  });

  it('overrides a model alone while the provider is inherited', async () => {
    mockClient.getWorkspaceAiSettings.mockResolvedValue(inherited);
    mockClient.getEffectiveAiSettings.mockResolvedValue({
      ...organizationSettings,
      model_fast: 'llama3.2:3b',
    });
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Override Fast Model' }));
    expect(selectOptionValues('Fast Model')).not.toContain('');
    chooseSelect('Fast Model', 'llama3.1:8b');
    await user.click(screen.getByRole('button', { name: 'Save Fast Model' }));

    const request = await sentRequest();
    expect(request).toEqual({ model_fast: 'llama3.1:8b' });
    expect(request).not.toHaveProperty('provider');
  });

  it('resends the workspace provider with a model override', async () => {
    const openai: WorkspaceAiSettings = {
      ...inherited,
      provider: 'openai',
      overrides: true,
      inherits_provider: false,
    };
    mockClient.getWorkspaceAiSettings.mockResolvedValue(openai);
    mockClient.getEffectiveAiSettings.mockResolvedValue({
      ...organizationSettings,
      provider: 'openai',
      model_fast: null,
    });
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Override Fast Model' }));
    chooseSelect('Fast Model', 'gpt-4o');
    await user.click(screen.getByRole('button', { name: 'Save Fast Model' }));

    expect(await sentRequest()).toEqual({ provider: 'openai', model_fast: 'gpt-4o' });
  });

  it('sends an empty video model to reset it to the organization', async () => {
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Reset Video Model' }));

    expect(await sentRequest()).toEqual({ provider: 'self_hosted', model_video: '' });
    expect(await screen.findByText('Video Model follows the organization')).toHaveClass(
      'alert-success'
    );
    expect(mockClient.resetWorkspaceAiSettings).not.toHaveBeenCalled();
    expect(mockClient.getEffectiveAiSettings).toHaveBeenCalledTimes(2);
  });

  it('asks before a provider reset removes saved credentials, naming them', async () => {
    mockClient.getWorkspaceAiSettings.mockResolvedValue({
      ...inherited,
      provider: 'bedrock',
      overrides: true,
      inherits_provider: false,
      has_runpod_api_key: true,
      bedrock_region: 'us-east-1',
      has_bedrock_credentials: true,
      model_video: 'wan2.2_ti2v_5B_fp16.safetensors',
    });
    mockClient.getEffectiveAiSettings.mockResolvedValue({
      ...organizationSettings,
      provider: 'bedrock',
    });
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Reset Provider' }));

    const dialog = screen.getByRole('dialog', { name: 'Reset Provider to the organization?' });
    expect(
      within(dialog)
        .getAllByRole('listitem')
        .map((item) => item.textContent)
    ).toEqual(['AWS Bedrock region', 'AWS Bedrock access keys']);
    expect(mockClient.updateWorkspaceAiSettings).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole('button', { name: 'Reset' }));

    expect(await sentRequest()).toEqual({
      model_fast: '',
      model_reasoning: '',
      model_embedding: '',
      bedrock_region: '',
      bedrock_access_key: '',
      bedrock_secret_key: '',
      bedrock_use_iam_role: null,
    });
  });

  it('cancels a provider reset without sending anything', async () => {
    mockClient.getWorkspaceAiSettings.mockResolvedValue({
      ...overridden,
      provider: 'openai',
      has_openai_api_key: true,
    });
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Reset Provider' }));
    const dialog = screen.getByRole('dialog', { name: 'Reset Provider to the organization?' });
    expect(within(dialog).getByText('OpenAI API key')).toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(mockClient.updateWorkspaceAiSettings).not.toHaveBeenCalled();
  });

  it('resets a provider that saved no credentials and no chat models in one click', async () => {
    mockClient.getWorkspaceAiSettings.mockResolvedValue({
      ...providerOnly,
      model_video: 'wan2.2_ti2v_5B_fp16.safetensors',
    });
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Reset Provider' }));

    expect(screen.queryByRole('dialog')).toBeNull();
    expect(await sentRequest()).toEqual({
      model_fast: '',
      model_reasoning: '',
      model_embedding: '',
    });
  });

  it('asks before a provider reset clears a Fast Model the workspace set, naming it', async () => {
    mockClient.getWorkspaceAiSettings.mockResolvedValue({
      ...providerOnly,
      model_fast: 'qwen2.5:7b',
    });
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Reset Provider' }));

    const dialog = screen.getByRole('dialog', { name: 'Reset Provider to the organization?' });
    expect(
      within(dialog)
        .getAllByRole('listitem')
        .map((item) => item.textContent)
    ).toEqual(['Fast Model: qwen2.5:7b']);
    expect(mockClient.updateWorkspaceAiSettings).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole('button', { name: 'Reset' }));

    expect(await sentRequest()).toEqual({
      model_fast: '',
      model_reasoning: '',
      model_embedding: '',
    });
  });

  it('names both the saved key and the chat model a provider reset removes', async () => {
    mockClient.getWorkspaceAiSettings.mockResolvedValue({
      ...providerOnly,
      provider: 'openai',
      has_openai_api_key: true,
      model_reasoning: 'o3-mini',
    });
    mockClient.getEffectiveAiSettings.mockResolvedValue({
      ...organizationSettings,
      provider: 'openai',
    });
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Reset Provider' }));

    const dialog = screen.getByRole('dialog', { name: 'Reset Provider to the organization?' });
    expect(
      within(dialog)
        .getAllByRole('listitem')
        .map((item) => item.textContent)
    ).toEqual(['Reasoning Model: o3-mini', 'OpenAI API key']);
  });

  it('trusts the save to drop the workspace row once the last override is reset', async () => {
    mockClient.getWorkspaceAiSettings.mockResolvedValue({
      ...inherited,
      overrides: true,
      model_video: 'wan2.2_ti2v_5B_fp16.safetensors',
    });
    mockClient.updateWorkspaceAiSettings.mockResolvedValue(inherited);
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Reset Video Model' }));

    expect(await sentRequest()).toEqual({ model_video: '' });
    await waitFor(() => expect(mockClient.getEffectiveAiSettings).toHaveBeenCalledTimes(2));
    expect(mockClient.resetWorkspaceAiSettings).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: RESET_ALL })).toBeNull();
  });

  it('says the save worked when only the refreshed values fail to load', async () => {
    mockClient.getEffectiveAiSettings
      .mockResolvedValueOnce(organizationSettings)
      .mockRejectedValueOnce(new Error('Network down'));
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Reset Video Model' }));

    expect(
      await screen.findByText(
        "Video Model follows the organization, but the refreshed values couldn't load. Reload the page to see them."
      )
    ).toHaveClass('alert-warning');
    expect(screen.queryByText('Failed to save AI settings')).toBeNull();
    expect(screen.queryByText('Network down')).toBeNull();
    expect(screen.getByRole('button', { name: 'Reset Image Model' })).toBeEnabled();
  });

  it('locks the other rows while one row is open', async () => {
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Edit Provider' }));
    chooseSelect('AI Provider', 'OpenAI');

    expect(screen.getByRole('button', { name: 'Edit Fast Model' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Reset Fast Model' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Override Runpod API key' })).toBeDisabled();
    expect(screen.getByRole('button', { name: RESET_ALL })).toBeDisabled();
    expect(screen.getByLabelText('AI Provider')).toHaveTextContent('OpenAI');

    await user.click(within(row('Provider')).getByRole('button', { name: 'Cancel' }));
    expect(screen.getByRole('button', { name: 'Edit Fast Model' })).toBeEnabled();
  });

  it('names the saved credentials Reset all removes', async () => {
    mockClient.getWorkspaceAiSettings.mockResolvedValue({
      ...inherited,
      overrides: true,
      has_runpod_api_key: true,
      litellm_host: 'http://litellm.local:4000',
      has_litellm_key: true,
    });
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: RESET_ALL }));

    const dialog = screen.getByRole('dialog', {
      name: 'Reset all AI settings to the organization?',
    });
    expect(
      within(dialog)
        .getAllByRole('listitem')
        .map((item) => item.textContent)
    ).toEqual(['Runpod API key', 'LiteLLM host', 'LiteLLM API key']);
  });

  it('offers Reset all only to an editor of a workspace that overrides', async () => {
    mockClient.getWorkspaceAiSettings.mockResolvedValue(inherited);
    await renderSection();

    expect(
      screen.queryByRole('button', { name: 'Reset all AI settings to the organization' })
    ).toBeNull();
  });

  it('hides Reset all from a reader of a workspace that overrides', async () => {
    await renderSection({ editable: false });

    expect(
      screen.queryByRole('button', { name: 'Reset all AI settings to the organization' })
    ).toBeNull();
  });

  it('resets everything to the organization and reloads the effective settings', async () => {
    mockClient.getEffectiveAiSettings
      .mockResolvedValueOnce(organizationSettings)
      .mockResolvedValueOnce({ ...organizationSettings, provider: 'claude_code' });
    const user = await renderSection();

    const reset = screen.getByRole('button', { name: RESET_ALL });
    expect(reset).toHaveTextContent('Reset all to organization');
    expect(reset.closest('.section-row-actions')).not.toBeNull();
    await user.click(reset);

    const dialog = screen.getByRole('dialog', {
      name: 'Reset all AI settings to the organization?',
    });
    expect(
      within(dialog)
        .getAllByRole('listitem')
        .map((item) => item.textContent)
    ).toEqual([
      'Provider',
      'Fast Model',
      'Reasoning Model',
      'Embedding Model',
      'Image Model',
      'Video Model',
      'Audio Model',
    ]);
    expect(mockClient.resetWorkspaceAiSettings).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole('button', { name: 'Reset all' }));

    expect(
      await within(row('Provider')).findByText('Claude Code (Claude subscription)')
    ).toBeInTheDocument();
    expect(mockClient.resetWorkspaceAiSettings).toHaveBeenCalledWith(ORGANIZATION_ID, WORKSPACE_ID);
    expect(mockClient.updateWorkspaceAiSettings).not.toHaveBeenCalled();
    expect(mockClient.getEffectiveAiSettings).toHaveBeenCalledTimes(2);
    expect(
      screen.queryByRole('button', { name: 'Reset all AI settings to the organization' })
    ).toBeNull();
  });

  describe('bedrock override', () => {
    const organizationBedrock: AiSettings = {
      ...organizationSettings,
      provider: 'bedrock',
      bedrock_region: 'eu-west-1',
      bedrock_use_iam_role: true,
      has_bedrock_credentials: false,
    };

    it("keeps the organization's region and IAM role the user left alone", async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue(inherited);
      mockClient.getEffectiveAiSettings.mockResolvedValue(organizationBedrock);
      const user = await renderSection();

      await user.click(screen.getByRole('button', { name: 'Override Provider' }));
      expect(screen.getByLabelText('AWS Region')).toHaveTextContent('eu-west-1');
      expect(screen.getByLabelText(/Use IAM Role/)).toBeChecked();
      await user.click(screen.getByRole('button', { name: 'Save Provider' }));

      expect(await sentRequest()).toEqual({ provider: 'bedrock' });
    });

    it('sends only the bedrock field the user changed', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue(inherited);
      mockClient.getEffectiveAiSettings.mockResolvedValue(organizationBedrock);
      const user = await renderSection();

      await user.click(screen.getByRole('button', { name: 'Override Provider' }));
      chooseSelect('AWS Region', 'us-west-2');
      await user.click(screen.getByRole('button', { name: 'Save Provider' }));

      expect(await sentRequest()).toEqual({ provider: 'bedrock', bedrock_region: 'us-west-2' });
    });
  });

  it('shows the reason the server refused the save', async () => {
    const message =
      'Failed to update workspace AI settings: litellm_host: Re-enter the key when changing the endpoint URL.';
    mockClient.updateWorkspaceAiSettings.mockRejectedValueOnce(new ApiError(message, 400));
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: 'Edit Provider' }));
    await user.click(screen.getByRole('button', { name: 'Save Provider' }));

    expect(await screen.findByText(message)).toHaveClass('alert-error');
    expect(screen.getByLabelText('AI Provider')).toBeInTheDocument();
  });

  it('shows the reason the server refused the reset', async () => {
    const message =
      'Failed to reset workspace AI settings: Only workspace admins can change AI settings';
    mockClient.resetWorkspaceAiSettings.mockRejectedValue(new ApiError(message, 403));
    const user = await renderSection();

    await user.click(screen.getByRole('button', { name: RESET_ALL }));
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Reset all' }));

    expect(await screen.findByText(message)).toHaveClass('alert-error');
  });

  it('shows a reader the values with no buttons', async () => {
    await renderSection({ editable: false });

    expect(screen.queryAllByRole('button')).toHaveLength(0);
    expect(screen.getByText('Only workspace admins can change AI settings.')).toBeInTheDocument();
    expect(within(row('Fast Model')).getByText('llama3.1:8b')).toBeInTheDocument();
  });

  it('links to the organization defaults only when given the path', async () => {
    const { unmount } = render(<WorkspaceAiSection editable />);
    await screen.findByRole('group', { name: 'Provider' });
    expect(screen.queryByRole('link', { name: 'Edit organization defaults' })).toBeNull();
    unmount();

    await renderSection({ organizationDefaultsPath: '/settings/organization/ai' });

    expect(screen.getByRole('link', { name: 'Edit organization defaults' })).toHaveAttribute(
      'href',
      '/settings/organization/ai'
    );
  });

  describe('coding agent overrides', () => {
    it('shows the sign-in of the coding agent the workspace chose', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue(codexOnly);
      mockClient.getEffectiveAiSettings.mockResolvedValue(codexEffective);
      const user = await renderSection();

      expect(within(row('Provider')).getByText('Set for this workspace')).toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'Edit Provider' }));

      expect(screen.getByLabelText('AI Provider')).toHaveTextContent(
        'Codex (ChatGPT subscription)'
      );
      expect(
        await screen.findByRole('heading', { name: 'Codex sign-in', level: 3 })
      ).toBeInTheDocument();
      expect(agentsApi.list).toHaveBeenCalledWith(ORGANIZATION_ID);
    });

    it('offers the coding agent models with no blank option', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue(codexOnly);
      mockClient.getEffectiveAiSettings.mockResolvedValue(codexEffective);
      const user = await renderSection();

      await user.click(screen.getByRole('button', { name: 'Override Fast Model' }));

      await waitFor(() =>
        expect(selectOptionValues('Fast Model')).toEqual(['gpt-6-astra', 'gpt-6-sol', 'gpt-6-luna'])
      );
    });

    it('does not ask for sign-in status while inheriting the organization', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue(inherited);
      mockClient.getEffectiveAiSettings.mockResolvedValue(codexEffective);
      await renderSection();

      expect(within(row('Provider')).getByText('Codex (ChatGPT subscription)')).toBeInTheDocument();
      expect(agentsApi.list).not.toHaveBeenCalled();
    });

    it('shows a member the organization sign-in with no code and no buttons', async () => {
      organizationRole = 'member';
      mockClient.getWorkspaceAiSettings.mockResolvedValue(codexOnly);
      const user = await renderSection();

      await user.click(screen.getByRole('button', { name: 'Edit Provider' }));

      const panel = (await screen.findByText('Codex sign-in')).closest('section');
      expect(panel).not.toBeNull();
      expect(await screen.findByText('Ask an organization admin to sign in.')).toBeInTheDocument();
      expect(panel?.querySelectorAll('button')).toHaveLength(0);
      expect(screen.queryByText('ABCD-EFGHI')).toBeNull();
    });

    describe('after the provider changes', () => {
      const [claudeStatus, codexStatus] = fixture.agents;
      const signedOut = [
        {
          ...claudeStatus,
          state: 'signed_out',
          source: null,
          label: null,
          expires_at: null,
          logins: [],
        },
        { ...codexStatus, state: 'signed_out', pending: null },
      ];
      const later = () => new Date(Date.now() + 10 * 60_000).toISOString();

      beforeEach(() => {
        organizationRole = 'owner';
        agentsApi.list.mockResolvedValue(signedOut);
      });

      it("shows Claude none of Codex's sign-in", async () => {
        mockClient.getWorkspaceAiSettings.mockResolvedValue(codexOnly);
        agentsApi.start.mockResolvedValue({
          agent: 'codex',
          verification_url: 'https://auth.openai.com/codex/device',
          user_code: 'ABCD-EFGHI',
          expires_at: later(),
        });
        agentsApi.cancel.mockRejectedValue(new Error('Failed to cancel the codex sign-in: 500'));
        const user = await renderSection();
        await user.click(screen.getByRole('button', { name: 'Edit Provider' }));

        await user.click(await screen.findByRole('button', { name: 'Sign in with ChatGPT' }));
        expect(await screen.findByText('ABCD-EFGHI')).toBeInTheDocument();
        const codexPanel = screen.getByRole('region', { name: 'Codex sign-in' });
        await user.click(within(codexPanel).getByRole('button', { name: 'Cancel' }));
        expect(await screen.findByRole('alert')).toHaveTextContent(
          'Failed to cancel the codex sign-in: 500'
        );

        chooseSelect('AI Provider', 'Claude Code (Claude subscription)');

        const panel = screen.getByRole('region', { name: 'Claude Code sign-in' });
        expect(within(panel).getByRole('button', { name: 'Sign in with Claude' })).toBeEnabled();
        expect(within(panel).getByText('Not signed in')).toBeInTheDocument();
        expect(within(panel).queryByRole('button', { name: 'Cancel' })).toBeNull();
        expect(screen.queryByText('ABCD-EFGHI')).toBeNull();
        expect(screen.queryByRole('alert')).toBeNull();
      });

      it("shows Codex none of Claude's sign-in", async () => {
        mockClient.getWorkspaceAiSettings.mockResolvedValue({
          ...codexOnly,
          provider: 'claude_code',
        });
        agentsApi.start.mockResolvedValue({
          agent: 'claude',
          authorize_url: 'https://claude.com/cai/oauth/authorize?code=true&state=fake-state',
          expires_at: later(),
          flow: 'paste',
        });
        agentsApi.submitCode.mockRejectedValue(
          new Error('Claude rejected the code: Invalid authorization code')
        );
        const user = await renderSection();
        await user.click(screen.getByRole('button', { name: 'Edit Provider' }));

        await user.click(await screen.findByRole('button', { name: 'Sign in with Claude' }));
        await user.type(
          await screen.findByLabelText('Code from claude.com'),
          'fake-code#fake-state'
        );
        await user.click(screen.getByRole('button', { name: 'Submit code' }));
        expect(await screen.findByRole('alert')).toHaveTextContent('Claude rejected the code');

        chooseSelect('AI Provider', 'Codex (ChatGPT subscription)');

        const panel = screen.getByRole('region', { name: 'Codex sign-in' });
        expect(within(panel).getByRole('button', { name: 'Sign in with ChatGPT' })).toBeEnabled();
        expect(screen.queryByRole('link', { name: 'Open claude.com' })).toBeNull();
        expect(screen.queryByLabelText('Code from claude.com')).toBeNull();
        expect(screen.queryByRole('alert')).toBeNull();
      });
    });

    it('shows the sign-in status alone while the role resolves, then fails closed', async () => {
      resolvingRole = true;
      mockClient.getWorkspaceAiSettings.mockResolvedValue({
        ...codexOnly,
        provider: 'claude_code',
      });
      agentsApi.list.mockResolvedValue([
        {
          ...fixture.agents[0],
          state: 'signed_out',
          source: null,
          label: null,
          expires_at: null,
          logins: [],
        },
        fixture.agents[1],
      ]);
      const user = userEvent.setup();
      const { rerender } = render(<WorkspaceAiSection editable />);
      await user.click(await screen.findByRole('button', { name: 'Edit Provider' }));

      const panel = await screen.findByRole('region', { name: 'Claude Code sign-in' });
      expect(await within(panel).findByText('Not signed in')).toBeInTheDocument();
      expect(within(panel).queryAllByRole('button')).toHaveLength(0);
      expect(within(panel).queryByText('Ask an organization admin to sign in.')).toBeNull();

      resolvingRole = false;
      rerender(<WorkspaceAiSection editable />);

      expect(within(panel).getByText('Ask an organization admin to sign in.')).toBeInTheDocument();
      expect(within(panel).queryAllByRole('button')).toHaveLength(0);
    });

    it('asks to save an override the organization is signed in for', async () => {
      organizationRole = 'owner';
      mockClient.getWorkspaceAiSettings.mockResolvedValue(inherited);
      const user = await renderSection();

      await user.click(screen.getByRole('button', { name: 'Override Provider' }));
      chooseSelect('AI Provider', 'Claude Code (Claude subscription)');

      expect(await screen.findByText('Save to use this provider.')).toBeInTheDocument();
    });

    it('tells a signed-out organization the sign-in covers every workspace', async () => {
      organizationRole = 'owner';
      agentsApi.list.mockResolvedValue([
        { ...fixture.agents[0], state: 'signed_out', source: null, label: null, logins: [] },
        fixture.agents[1],
      ]);
      mockClient.getWorkspaceAiSettings.mockResolvedValue(inherited);
      const user = await renderSection();

      await user.click(screen.getByRole('button', { name: 'Override Provider' }));
      chooseSelect('AI Provider', 'Claude Code (Claude subscription)');

      expect(
        await screen.findByText(
          'Sign in with a Claude subscription to use Claude Code. Signing in here signs in for every workspace in this organization.'
        )
      ).toBeInTheDocument();
    });

    it('asks nothing while overriding with the provider the workspace already inherits', async () => {
      organizationRole = 'owner';
      mockClient.getWorkspaceAiSettings.mockResolvedValue(inherited);
      mockClient.getEffectiveAiSettings.mockResolvedValue({
        ...organizationSettings,
        provider: 'claude_code',
      });
      const user = await renderSection();

      await user.click(screen.getByRole('button', { name: 'Override Provider' }));

      const panel = await screen.findByRole('region', { name: 'Claude Code sign-in' });
      expect(
        await within(within(panel).getByRole('status')).findByText('Signed in')
      ).toBeInTheDocument();
      expect(screen.queryByText('Save to use this provider.')).toBeNull();
    });

    it('asks nothing of an override already saved', async () => {
      organizationRole = 'owner';
      mockClient.getWorkspaceAiSettings.mockResolvedValue({
        ...codexOnly,
        provider: 'claude_code',
      });
      const user = await renderSection();

      await user.click(screen.getByRole('button', { name: 'Edit Provider' }));

      const panel = await screen.findByRole('region', { name: 'Claude Code sign-in' });
      expect(
        await within(within(panel).getByRole('status')).findByText('Signed in')
      ).toBeInTheDocument();
      expect(screen.queryByText('Save to use this provider.')).toBeNull();
    });

    it('saves the coding agent override without credentials', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue(codexOnly);
      mockClient.getEffectiveAiSettings.mockResolvedValue(codexEffective);
      mockClient.updateWorkspaceAiSettings.mockResolvedValue(codexOnly);
      const user = await renderSection();

      await user.click(screen.getByRole('button', { name: 'Edit Provider' }));
      await screen.findByText('Codex sign-in');
      await user.click(screen.getByRole('button', { name: 'Save Provider' }));

      expect(await sentRequest()).toEqual({ provider: 'codex' });
    });
  });
});
