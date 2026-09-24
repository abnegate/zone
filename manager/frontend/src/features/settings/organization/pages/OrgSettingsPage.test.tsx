import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  mock,
  setSystemTime,
} from 'bun:test';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import fixture from '../../../../../../../runner/zone_server/tests/fixtures/agents.json';
import type { AiSettings, OrgRole } from '../types';

// Mock client
const mockClient = {
  getOrgAiSettings: mock(),
  updateOrgAiSettings: mock(),
  resetOrgAiSettings: mock(),
  getOrgMembers: mock(),
  getWorkspaces: mock(),
};

mock.module('../../../../api/client', () => ({
  client: mockClient,
}));

const agentsApi = {
  list: mock(),
  get: mock(),
  start: mock(),
  submitCode: mock(),
  signOut: mock(),
};

mock.module('../../../../api/agents', () => ({ agentsApi }));

// Mock OrgMembersSection, InvitationsSection, BillingSection, and AuditLogsSection components
mock.module('../components', () => ({
  Button: ({ children, ...props }: React.ComponentProps<'button'>) => (
    <button {...props}>{children}</button>
  ),
  OrgMembersSection: ({ orgId }: { orgId: string }) => (
    <div data-testid="org-members-section">OrgMembersSection: {orgId}</div>
  ),
  InvitationsSection: ({ orgId, workspaces }: { orgId: string; workspaces: unknown[] }) => (
    <div data-testid="invitations-section">
      InvitationsSection: {orgId}, workspaces: {workspaces.length}
    </div>
  ),
  BillingSection: ({ orgId }: { orgId: string }) => (
    <div data-testid="billing-section">BillingSection: {orgId}</div>
  ),
  AuditLogsSection: ({ orgId }: { orgId: string }) => (
    <div data-testid="audit-logs-section">AuditLogsSection: {orgId}</div>
  ),
}));

// Mock useAuth
const mockUseAuth = mock(() => ({
  isAuthenticated: true,
  user: { id: '1', email: 'test@test.com' },
}));

mock.module('../../../auth', () => ({
  useAuth: mockUseAuth,
}));

let installedModels: unknown[] = [];

mock.module('../../../models', () => ({
  useModels: () => ({
    models: installedModels,
    loading: false,
    error: null,
    refresh: mock(),
    deleteModel: mock(),
  }),
}));

// Mock useWorkspace
const mockSetCurrentOrganization = mock();
const mockSetCurrentWorkspace = mock();
const mockRefreshOrganizations = mock();
const mockRefreshWorkspaces = mock();

const mockCurrentOrganization = {
  id: '00000000-0000-0000-0000-000000000001',
  name: 'Test Org',
  slug: 'test-org',
  description: null,
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

const mockWorkspaceContext: {
  currentOrganization: typeof mockCurrentOrganization & { role?: OrgRole };
  [key: string]: unknown;
} = {
  currentOrganization: mockCurrentOrganization,
  organizations: [],
  currentWorkspace: null,
  workspaces: [],
  loading: false,
  error: null,
  setCurrentOrganization: mockSetCurrentOrganization,
  setCurrentWorkspace: mockSetCurrentWorkspace,
  refreshOrganizations: mockRefreshOrganizations,
  refreshWorkspaces: mockRefreshWorkspaces,
};

mock.module('../../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => mockWorkspaceContext,
}));

let OrgSettingsPage: typeof import('./OrgSettingsPage').default;

beforeAll(async () => {
  OrgSettingsPage = (await import('./OrgSettingsPage')).default;
});

afterAll(() => {
  mock.restore();
});

const mockAiSettings: AiSettings = {
  provider: 'self_hosted',
  has_litellm_key: false,
  litellm_host: 'http://localhost:4000',
  has_openai_api_key: false,
  openai_base_url: null,
  has_anthropic_api_key: false,
  anthropic_base_url: null,
  bedrock_region: null,
  bedrock_use_iam_role: false,
  has_bedrock_credentials: false,
  model_fast: 'llama3.1:8b',
  model_reasoning: 'deepseek-r1:7b',
  model_embedding: 'nomic-embed-text',
  model_image: 'flux1-schnell-fp8.safetensors',
  model_video: 'wan2.2_ti2v_5B_fp16.safetensors',
  model_audio: 'ace_step_v1_3.5b.safetensors',
};

describe('OrgSettingsPage', () => {
  beforeEach(() => {
    mock.clearAllMocks();
    installedModels = [];
    mockWorkspaceContext.currentOrganization = mockCurrentOrganization;
    mockClient.getOrgAiSettings.mockResolvedValue(mockAiSettings);
    mockClient.getWorkspaces.mockResolvedValue([]);
    agentsApi.list.mockResolvedValue(fixture.agents);
  });

  describe('Loading State', () => {
    it('shows loading state initially', () => {
      mockClient.getOrgAiSettings.mockImplementation(() => new Promise(() => {}));
      render(<OrgSettingsPage />);
      expect(screen.getByText('Loading settings...')).toBeInTheDocument();
    });

    it('shows error when loading fails', async () => {
      mockClient.getOrgAiSettings.mockRejectedValueOnce(new Error('Failed to load'));
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByText('Failed to load')).toBeInTheDocument();
      });
    });
  });

  describe('Page Structure', () => {
    it('renders page header', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByText('Organization Settings')).toBeInTheDocument();
      });
    });

    it('renders AI Provider Configuration section', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByText('AI Provider Configuration')).toBeInTheDocument();
      });
    });

    it('renders save and reset buttons', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Save Changes' })).toBeInTheDocument();
        expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeInTheDocument();
      });
    });
  });

  describe('Provider Selection', () => {
    it('renders provider dropdown with current value', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        const select = screen.getByLabelText('AI Provider');
        expect(select).toHaveValue('self_hosted');
      });
    });

    it('changes provider when dropdown changes', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByLabelText('AI Provider')).toBeInTheDocument();
      });

      const select = screen.getByLabelText('AI Provider');
      fireEvent.change(select, { target: { value: 'openai' } });

      // The select should update immediately since it's controlled
      expect(select).toHaveValue('openai');
    });

    it('shows all provider options', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByLabelText('AI Provider')).toBeInTheDocument();
      });

      const select = screen.getByLabelText('AI Provider');
      const options = select.querySelectorAll('option');
      expect(Array.from(options, (option) => option.value)).toEqual([
        'self_hosted',
        'openai',
        'anthropic',
        'bedrock',
        'claude_code',
        'codex',
      ]);
    });
  });

  describe('Coding Agent Providers', () => {
    const agentSettings: AiSettings = {
      ...mockAiSettings,
      provider: 'claude_code',
      model_fast: null,
      model_reasoning: 'opus',
      model_embedding: null,
    };

    beforeEach(() => {
      setSystemTime(new Date('2026-09-23T04:00:00Z'));
    });

    afterEach(() => {
      setSystemTime();
    });

    it('asks for sign-in status only once a coding agent is chosen', async () => {
      mockWorkspaceContext.currentOrganization = { ...mockCurrentOrganization, role: 'owner' };
      render(<OrgSettingsPage />);
      const select = await screen.findByLabelText('AI Provider');
      expect(agentsApi.list).not.toHaveBeenCalled();
      expect(screen.queryByText('Claude Code sign-in')).toBeNull();

      fireEvent.change(select, { target: { value: 'claude_code' } });

      expect(
        await screen.findByRole('heading', { name: 'Claude Code sign-in', level: 4 })
      ).toBeInTheDocument();
      expect(agentsApi.list).toHaveBeenCalledWith(mockCurrentOrganization.id);
      expect(screen.queryByLabelText(/LiteLLM Host/i)).toBeNull();
      expect(screen.getByText('Signed in')).toBeInTheDocument();

      fireEvent.change(select, { target: { value: 'codex' } });

      expect(await screen.findByText('Codex sign-in')).toBeInTheDocument();
      expect(screen.getByText('ABCD-EFGHI')).toBeInTheDocument();
      expect(agentsApi.list).toHaveBeenCalledTimes(1);
    });

    it('lists the models the agent offers instead of the installed ones', async () => {
      installedModels = [
        { name: 'llama3.2:3b', size: 1, modified_at: '2024-01-01T00:00:00Z' },
        {
          name: 'nomic-embed-text:latest',
          size: 1,
          modified_at: '2024-01-01T00:00:00Z',
          capabilities: ['embeddings'],
        },
      ];
      mockClient.getOrgAiSettings.mockResolvedValue(agentSettings);
      render(<OrgSettingsPage />);

      await waitFor(() =>
        expect(
          Array.from(
            (screen.getByLabelText('Fast Model') as HTMLSelectElement).options,
            (option) => option.value
          )
        ).toEqual(['', 'sonnet', 'opus', 'haiku'])
      );
      expect(
        Array.from(
          (screen.getByLabelText('Reasoning Model') as HTMLSelectElement).options,
          (option) => option.value
        )
      ).toEqual(['', 'sonnet', 'opus', 'haiku']);
      expect(screen.getByLabelText('Reasoning Model')).toHaveValue('opus');
      expect(screen.getByText(/this server's own embedding engine/)).toBeInTheDocument();
    });

    it('names no other provider as the embedding default when none are installed', async () => {
      mockClient.getOrgAiSettings.mockResolvedValue(agentSettings);
      render(<OrgSettingsPage />);

      const embedding = await screen.findByLabelText('Embedding Model');
      expect(embedding).toHaveAttribute('placeholder', 'Server default');
      expect(screen.getByText(/this server's own embedding engine/)).toBeInTheDocument();
    });

    it('keeps a saved model the agent does not list so the form shows what is stored', async () => {
      mockClient.getOrgAiSettings.mockResolvedValue({
        ...agentSettings,
        model_fast: 'claude-sonnet-4-5',
      });
      render(<OrgSettingsPage />);

      await waitFor(() =>
        expect(screen.getByLabelText('Fast Model')).toHaveValue('claude-sonnet-4-5')
      );
    });

    it('saves a coding agent provider without any credentials, its models back on Automatic', async () => {
      mockClient.updateOrgAiSettings.mockResolvedValue(agentSettings);
      render(<OrgSettingsPage />);
      const select = await screen.findByLabelText('AI Provider');
      expect(screen.getByLabelText('Fast Model')).toHaveValue('llama3.1:8b');
      fireEvent.change(select, { target: { value: 'claude_code' } });

      await waitFor(() => expect(screen.getByLabelText('Fast Model')).toHaveValue(''));
      expect(screen.getByLabelText('Reasoning Model')).toHaveValue('');
      expect(
        Array.from(
          (screen.getByLabelText('Fast Model') as HTMLSelectElement).options,
          (option) => option.value
        )
      ).toEqual(['', 'sonnet', 'opus', 'haiku']);
      fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));

      await waitFor(() => expect(mockClient.updateOrgAiSettings).toHaveBeenCalled());
      const [organizationId, request] = mockClient.updateOrgAiSettings.mock.calls[0];
      expect(organizationId).toBe(mockCurrentOrganization.id);
      expect(request).toEqual({
        provider: 'claude_code',
        model_fast: '',
        model_reasoning: '',
        model_embedding: 'nomic-embed-text',
        model_image: 'flux1-schnell-fp8.safetensors',
        model_video: 'wan2.2_ti2v_5B_fp16.safetensors',
        model_audio: 'ace_step_v1_3.5b.safetensors',
      });
    });

    it('sends an empty model to clear one saved before Automatic was picked', async () => {
      mockClient.getOrgAiSettings.mockResolvedValue(agentSettings);
      mockClient.updateOrgAiSettings.mockResolvedValue({ ...agentSettings, model_reasoning: null });
      render(<OrgSettingsPage />);

      const reasoning = await screen.findByLabelText('Reasoning Model');
      await waitFor(() => expect(reasoning).toHaveValue('opus'));
      fireEvent.change(reasoning, { target: { value: '' } });
      fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));

      await waitFor(() => expect(mockClient.updateOrgAiSettings).toHaveBeenCalled());
      const [, request] = mockClient.updateOrgAiSettings.mock.calls[0];
      expect(request.model_fast).toBe('');
      expect(request.model_reasoning).toBe('');
      expect(request.model_embedding).toBe('');
    });

    it('says Automatic lets the agent choose, titles and summaries included', async () => {
      mockClient.getOrgAiSettings.mockResolvedValue(agentSettings);
      render(<OrgSettingsPage />);

      const fast = await screen.findByLabelText('Fast Model');
      expect(fast.closest('.form-group')?.querySelector('.form-hint')?.textContent).toBe(
        'Automatic lets the agent choose; titles, PR subjects and summaries use it too.'
      );
      const reasoning = screen.getByLabelText('Reasoning Model');
      expect(reasoning.closest('.form-group')?.querySelector('.form-hint')?.textContent).toBe(
        'Harder questions; empty lets the agent choose.'
      );
    });

    describe('after the provider changes', () => {
      const [claudeStatus, codexStatus] = fixture.agents;
      const signedOut = [
        { ...claudeStatus, state: 'signed_out', source: null, label: null, expires_at: null },
        { ...codexStatus, state: 'signed_out', pending: null },
      ];
      const later = () => new Date(Date.now() + 10 * 60_000).toISOString();

      beforeEach(() => {
        mockWorkspaceContext.currentOrganization = { ...mockCurrentOrganization, role: 'owner' };
        agentsApi.list.mockResolvedValue(signedOut);
      });

      it("shows Claude none of Codex's sign-in", async () => {
        mockClient.getOrgAiSettings.mockResolvedValue({ ...agentSettings, provider: 'codex' });
        agentsApi.start.mockResolvedValue({
          agent: 'codex',
          verification_url: 'https://auth.openai.com/codex/device',
          user_code: 'ABCD-EFGHI',
          expires_at: later(),
        });
        agentsApi.signOut.mockRejectedValue(new Error('Failed to sign out of codex: 500'));
        render(<OrgSettingsPage />);

        fireEvent.click(await screen.findByRole('button', { name: 'Sign in with ChatGPT' }));
        expect(await screen.findByText('ABCD-EFGHI')).toBeInTheDocument();
        fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
        expect(await screen.findByRole('alert')).toHaveTextContent(
          'Failed to sign out of codex: 500'
        );

        fireEvent.change(screen.getByLabelText('AI Provider'), {
          target: { value: 'claude_code' },
        });

        const panel = screen.getByRole('region', { name: 'Claude Code sign-in' });
        expect(within(panel).getByRole('button', { name: 'Sign in with Claude' })).toBeEnabled();
        expect(within(panel).getByText('Not signed in')).toBeInTheDocument();
        expect(screen.queryByText('ABCD-EFGHI')).toBeNull();
        expect(screen.queryByRole('button', { name: 'Cancel' })).toBeNull();
        expect(screen.queryByRole('alert')).toBeNull();
      });

      it("shows Codex none of Claude's sign-in", async () => {
        mockClient.getOrgAiSettings.mockResolvedValue(agentSettings);
        agentsApi.start.mockResolvedValue({
          agent: 'claude',
          authorize_url: 'https://claude.com/cai/oauth/authorize?code=true&state=fake-state',
          expires_at: later(),
        });
        agentsApi.submitCode.mockRejectedValue(
          new Error('Claude rejected the code: Invalid authorization code')
        );
        render(<OrgSettingsPage />);

        fireEvent.click(await screen.findByRole('button', { name: 'Sign in with Claude' }));
        fireEvent.change(await screen.findByLabelText('Code from claude.com'), {
          target: { value: 'fake-code#fake-state' },
        });
        fireEvent.click(screen.getByRole('button', { name: 'Submit code' }));
        expect(await screen.findByRole('alert')).toHaveTextContent('Claude rejected the code');

        fireEvent.change(screen.getByLabelText('AI Provider'), { target: { value: 'codex' } });

        const panel = screen.getByRole('region', { name: 'Codex sign-in' });
        expect(within(panel).getByRole('button', { name: 'Sign in with ChatGPT' })).toBeEnabled();
        expect(screen.queryByRole('link', { name: 'Open claude.com' })).toBeNull();
        expect(screen.queryByLabelText('Code from claude.com')).toBeNull();
        expect(screen.queryByRole('alert')).toBeNull();
      });
    });

    it('asks to save a provider the organization is signed in to but has not chosen yet', async () => {
      mockWorkspaceContext.currentOrganization = { ...mockCurrentOrganization, role: 'owner' };
      mockClient.updateOrgAiSettings.mockResolvedValue(agentSettings);
      render(<OrgSettingsPage />);

      fireEvent.change(await screen.findByLabelText('AI Provider'), {
        target: { value: 'claude_code' },
      });
      expect(await screen.findByText('Save Changes to use this provider.')).toBeInTheDocument();

      fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));

      expect(await screen.findByText('Settings saved successfully')).toBeInTheDocument();
      expect(screen.queryByText('Save Changes to use this provider.')).toBeNull();
    });

    it('keeps a Claude sign-in in flight while another tab is open', async () => {
      const [claudeStatus, codexStatus] = fixture.agents;
      const link = 'https://claude.com/cai/oauth/authorize?code=true&state=kept-state';
      mockWorkspaceContext.currentOrganization = { ...mockCurrentOrganization, role: 'owner' };
      mockClient.getOrgAiSettings.mockResolvedValue(agentSettings);
      agentsApi.list.mockResolvedValue([
        { ...claudeStatus, state: 'signed_out', source: null, label: null, expires_at: null },
        codexStatus,
      ]);
      agentsApi.start.mockResolvedValue({
        agent: 'claude',
        authorize_url: link,
        expires_at: new Date(Date.now() + 10 * 60_000).toISOString(),
      });
      render(<OrgSettingsPage />);

      fireEvent.click(await screen.findByRole('button', { name: 'Sign in with Claude' }));
      expect(await screen.findByRole('link', { name: 'Open claude.com' })).toHaveAttribute(
        'href',
        link
      );

      fireEvent.mouseDown(screen.getByRole('tab', { name: 'Members' }), { button: 0 });
      expect(screen.queryByRole('link', { name: 'Open claude.com' })).toBeNull();
      fireEvent.mouseDown(screen.getByRole('tab', { name: 'AI Settings' }), { button: 0 });

      expect(await screen.findByRole('link', { name: 'Open claude.com' })).toHaveAttribute(
        'href',
        link
      );
      expect(agentsApi.start).toHaveBeenCalledTimes(1);
    });

    it('lets an owner sign in and shows a member whom to ask', async () => {
      mockClient.getOrgAiSettings.mockResolvedValue(agentSettings);
      agentsApi.list.mockResolvedValue([
        { ...fixture.agents[0], state: 'signed_out', source: null, label: null, expires_at: null },
        fixture.agents[1],
      ]);
      mockWorkspaceContext.currentOrganization = { ...mockCurrentOrganization, role: 'owner' };
      const { unmount } = render(<OrgSettingsPage />);
      expect(await screen.findByRole('button', { name: 'Sign in with Claude' })).toBeEnabled();
      unmount();

      mockWorkspaceContext.currentOrganization = { ...mockCurrentOrganization, role: 'member' };
      render(<OrgSettingsPage />);
      expect(await screen.findByText('Ask an organization admin to sign in.')).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Sign in with Claude' })).toBeNull();
    });
  });

  describe('Self-Hosted Provider', () => {
    it('shows LiteLLM configuration for self_hosted provider', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByLabelText(/LiteLLM Host/i)).toBeInTheDocument();
      });
    });

    it('displays LiteLLM host value from settings', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByLabelText(/LiteLLM Host/i)).toHaveValue('http://localhost:4000');
      });
    });
  });

  describe('OpenAI Provider', () => {
    it('shows OpenAI configuration when openai provider selected', async () => {
      mockClient.getOrgAiSettings.mockResolvedValueOnce({
        ...mockAiSettings,
        provider: 'openai',
        has_openai_api_key: true,
      });

      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByLabelText(/OpenAI API Key/i)).toBeInTheDocument();
      });
    });

    it('shows configured indicator when API key is set', async () => {
      mockClient.getOrgAiSettings.mockResolvedValueOnce({
        ...mockAiSettings,
        provider: 'openai',
        has_openai_api_key: true,
      });

      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByText(/\(configured\)/i)).toBeInTheDocument();
      });
    });
  });

  describe('Anthropic Provider', () => {
    it('shows Anthropic configuration when anthropic provider selected', async () => {
      mockClient.getOrgAiSettings.mockResolvedValueOnce({
        ...mockAiSettings,
        provider: 'anthropic',
      });

      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByLabelText(/Anthropic API Key/i)).toBeInTheDocument();
      });
    });
  });

  describe('AWS Bedrock Provider', () => {
    it('shows Bedrock configuration when bedrock provider selected', async () => {
      mockClient.getOrgAiSettings.mockResolvedValueOnce({
        ...mockAiSettings,
        provider: 'bedrock',
        bedrock_region: 'us-east-1',
      });

      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByLabelText(/AWS Region/i)).toBeInTheDocument();
        expect(screen.getByLabelText(/Use IAM Role/i)).toBeInTheDocument();
      });
    });

    it('shows credential fields when IAM role is not used', async () => {
      mockClient.getOrgAiSettings.mockResolvedValueOnce({
        ...mockAiSettings,
        provider: 'bedrock',
        bedrock_region: 'us-east-1',
        bedrock_use_iam_role: false,
      });

      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByLabelText(/Access Key ID/i)).toBeInTheDocument();
        expect(screen.getByLabelText(/Secret Access Key/i)).toBeInTheDocument();
      });
    });

    it('hides credential fields when IAM role is used', async () => {
      mockClient.getOrgAiSettings.mockResolvedValueOnce({
        ...mockAiSettings,
        provider: 'bedrock',
        bedrock_region: 'us-east-1',
        bedrock_use_iam_role: true,
      });

      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.queryByLabelText(/Access Key ID/i)).not.toBeInTheDocument();
        expect(screen.queryByLabelText(/Secret Access Key/i)).not.toBeInTheDocument();
      });
    });
  });

  describe('Model Selection', () => {
    it('shows model selection section', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByText('Default Models')).toBeInTheDocument();
      });
    });

    it('shows fast model dropdown', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByLabelText('Fast Model')).toBeInTheDocument();
      });
    });

    it('shows reasoning model dropdown', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByLabelText('Reasoning Model')).toBeInTheDocument();
      });
    });

    it('shows embedding model dropdown', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByLabelText('Embedding Model')).toBeInTheDocument();
      });
    });

    it('displays current model values', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByLabelText('Fast Model')).toHaveValue('llama3.1:8b');
        expect(screen.getByLabelText('Reasoning Model')).toHaveValue('deepseek-r1:7b');
        expect(screen.getByLabelText('Embedding Model')).toHaveValue('nomic-embed-text');
      });
    });
  });

  describe('Save Functionality', () => {
    it('saves settings on form submit', async () => {
      mockClient.updateOrgAiSettings.mockResolvedValueOnce(mockAiSettings);

      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Save Changes' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));

      await waitFor(() => {
        expect(mockClient.updateOrgAiSettings).toHaveBeenCalled();
      });
    });

    it('sends an empty video model to clear the server-default override', async () => {
      mockClient.updateOrgAiSettings.mockResolvedValueOnce({
        ...mockAiSettings,
        model_video: null,
        model_audio: null,
      });

      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByLabelText('Video Model')).toHaveValue('wan2.2_ti2v_5B_fp16.safetensors');
      });

      fireEvent.change(screen.getByLabelText('Video Model'), { target: { value: '' } });
      fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));

      await waitFor(() => {
        expect(mockClient.updateOrgAiSettings).toHaveBeenCalledWith(
          expect.any(String),
          expect.objectContaining({ model_video: '' })
        );
      });
    });

    it('sends an empty audio model to clear the server-default override', async () => {
      mockClient.updateOrgAiSettings.mockResolvedValueOnce({
        ...mockAiSettings,
        model_audio: null,
      });

      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByLabelText('Audio Model')).toHaveValue('ace_step_v1_3.5b.safetensors');
      });

      fireEvent.change(screen.getByLabelText('Audio Model'), { target: { value: '' } });
      fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));

      await waitFor(() => {
        expect(mockClient.updateOrgAiSettings).toHaveBeenCalledWith(
          expect.any(String),
          expect.objectContaining({ model_audio: '' })
        );
      });
    });

    it('shows success message after save', async () => {
      mockClient.updateOrgAiSettings.mockResolvedValueOnce(mockAiSettings);

      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Save Changes' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));

      await waitFor(() => {
        expect(screen.getByText('Settings saved successfully')).toBeInTheDocument();
      });
    });

    it('shows error when save fails', async () => {
      mockClient.updateOrgAiSettings.mockRejectedValueOnce(new Error('Save failed'));

      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Save Changes' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));

      await waitFor(() => {
        expect(screen.getByText('Save failed')).toBeInTheDocument();
      });
    });
  });

  describe('Reset Functionality', () => {
    it('resets settings on reset button click', async () => {
      mockClient.resetOrgAiSettings.mockResolvedValueOnce(mockAiSettings);

      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Reset to Defaults' }));

      await waitFor(() => {
        expect(mockClient.resetOrgAiSettings).toHaveBeenCalled();
      });
    });

    it('shows success message after reset', async () => {
      mockClient.resetOrgAiSettings.mockResolvedValueOnce(mockAiSettings);

      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Reset to Defaults' }));

      await waitFor(() => {
        expect(screen.getByText('Settings reset to defaults')).toBeInTheDocument();
      });
    });

    it('shows error when reset fails', async () => {
      mockClient.resetOrgAiSettings.mockRejectedValueOnce(new Error('Reset failed'));

      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Reset to Defaults' }));

      await waitFor(() => {
        expect(screen.getByText('Reset failed')).toBeInTheDocument();
      });
    });
  });

  describe('Button States', () => {
    it('buttons are initially enabled', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Save Changes' })).toBeInTheDocument();
      });

      expect(screen.getByRole('button', { name: 'Save Changes' })).toBeEnabled();
      expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeEnabled();
    });
  });

  describe('Tab Navigation', () => {
    it('shows AI Settings tab by default', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByText('AI Provider Configuration')).toBeInTheDocument();
      });
    });

    it('switches to Members tab', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Members' })).toBeInTheDocument();
      });

      fireEvent.mouseDown(screen.getByRole('tab', { name: 'Members' }), {
        button: 0,
        ctrlKey: false,
      });

      expect(screen.getByTestId('org-members-section')).toBeInTheDocument();
      expect(screen.queryByText('AI Provider Configuration')).not.toBeInTheDocument();
    });

    it('switches to Invitations tab', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Invitations' })).toBeInTheDocument();
      });

      fireEvent.mouseDown(screen.getByRole('tab', { name: 'Invitations' }), {
        button: 0,
        ctrlKey: false,
      });

      expect(screen.getByTestId('invitations-section')).toBeInTheDocument();
      expect(screen.queryByText('AI Provider Configuration')).not.toBeInTheDocument();
    });

    it('switches to Billing tab', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Billing' })).toBeInTheDocument();
      });

      fireEvent.mouseDown(screen.getByRole('tab', { name: 'Billing' }), {
        button: 0,
        ctrlKey: false,
      });

      expect(screen.getByTestId('billing-section')).toBeInTheDocument();
      expect(screen.queryByText('AI Provider Configuration')).not.toBeInTheDocument();
    });

    it('switches to Audit Logs tab', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Audit Logs' })).toBeInTheDocument();
      });

      fireEvent.mouseDown(screen.getByRole('tab', { name: 'Audit Logs' }), {
        button: 0,
        ctrlKey: false,
      });

      expect(screen.getByTestId('audit-logs-section')).toBeInTheDocument();
      expect(screen.queryByText('AI Provider Configuration')).not.toBeInTheDocument();
    });

    it('renders all tab buttons', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'AI Settings' })).toBeInTheDocument();
      });

      expect(screen.getByRole('tab', { name: 'AI Settings' })).toBeInTheDocument();
      expect(screen.getByRole('tab', { name: 'Members' })).toBeInTheDocument();
      expect(screen.getByRole('tab', { name: 'Invitations' })).toBeInTheDocument();
      expect(screen.getByRole('tab', { name: 'Billing' })).toBeInTheDocument();
      expect(screen.getByRole('tab', { name: 'Audit Logs' })).toBeInTheDocument();
    });

    it('sets correct aria-selected on active tab', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'AI Settings' })).toBeInTheDocument();
      });

      const aiTab = screen.getByRole('tab', { name: 'AI Settings' });
      expect(aiTab).toHaveAttribute('aria-selected', 'true');

      fireEvent.keyDown(screen.getByRole('tab', { name: 'Audit Logs' }), { key: 'Enter' });

      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Audit Logs' })).toHaveAttribute(
          'aria-selected',
          'true'
        );
      });

      expect(aiTab).toHaveAttribute('aria-selected', 'false');
    });
  });
});
