import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { client } from '../../../../api/client';
import type { AiSettings } from '../types';
import OrgSettingsPage from './OrgSettingsPage';

// Mock client
jest.mock('../../../../api/client', () => ({
  client: {
    getOrgAiSettings: jest.fn(),
    updateOrgAiSettings: jest.fn(),
    resetOrgAiSettings: jest.fn(),
    getOrgMembers: jest.fn(),
    getWorkspaces: jest.fn(),
  },
}));

// Mock OrgMembersSection, InvitationsSection, BillingSection, and AuditLogsSection components
jest.mock('../components', () => ({
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
jest.mock('../../../auth', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    user: { id: '1', email: 'test@test.com' },
  }),
}));

// Mock useWorkspace
jest.mock('../../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentOrganization: {
      id: '00000000-0000-0000-0000-000000000001',
      name: 'Test Org',
      slug: 'test-org',
      description: null,
      is_active: true,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    },
    organizations: [],
    currentWorkspace: null,
    workspaces: [],
    loading: false,
    error: null,
    setCurrentOrganization: jest.fn(),
    setCurrentWorkspace: jest.fn(),
    refreshOrganizations: jest.fn(),
    refreshWorkspaces: jest.fn(),
  }),
}));

const mockClient = client as jest.Mocked<typeof client>;

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
};

describe('OrgSettingsPage', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    mockClient.getOrgAiSettings.mockResolvedValue(mockAiSettings);
    mockClient.getWorkspaces.mockResolvedValue([]);
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
      expect(options).toHaveLength(4);
    });
  });

  describe('Self-Hosted Provider', () => {
    it('shows LiteLLM configuration for self_hosted provider', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByText('LiteLLM Configuration')).toBeInTheDocument();
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
        expect(screen.getByText('OpenAI Configuration')).toBeInTheDocument();
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
        expect(screen.getByText(/\(set\)/i)).toBeInTheDocument();
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
        expect(screen.getByText('Anthropic Configuration')).toBeInTheDocument();
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
        expect(screen.getByLabelText(/Access Key/i)).toBeInTheDocument();
        expect(screen.getByLabelText(/Secret Key/i)).toBeInTheDocument();
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
        expect(screen.queryByLabelText(/Access Key/i)).not.toBeInTheDocument();
        expect(screen.queryByLabelText(/Secret Key/i)).not.toBeInTheDocument();
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

      fireEvent.click(screen.getByRole('tab', { name: 'Members' }));

      expect(screen.getByTestId('org-members-section')).toBeInTheDocument();
      expect(screen.queryByText('AI Provider Configuration')).not.toBeInTheDocument();
    });

    it('switches to Invitations tab', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Invitations' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('tab', { name: 'Invitations' }));

      expect(screen.getByTestId('invitations-section')).toBeInTheDocument();
      expect(screen.queryByText('AI Provider Configuration')).not.toBeInTheDocument();
    });

    it('switches to Billing tab', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Billing' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('tab', { name: 'Billing' }));

      expect(screen.getByTestId('billing-section')).toBeInTheDocument();
      expect(screen.queryByText('AI Provider Configuration')).not.toBeInTheDocument();
    });

    it('switches to Audit Logs tab', async () => {
      render(<OrgSettingsPage />);
      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Audit Logs' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('tab', { name: 'Audit Logs' }));

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

      fireEvent.click(screen.getByRole('tab', { name: 'Audit Logs' }));

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
