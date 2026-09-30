import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import type { Project, SyncConfig } from '../types';

const mockGetProjects = mock();
const mockGetSyncConfigs = mock();
const mockCreateSyncConfig = mock();
const mockDeleteSyncConfig = mock();
const mockSetWebhookSecret = mock();
const mockCreateProject = mock();
const mockUpdateProject = mock();
const mockDeleteProject = mock();
const mockGetSources = mock();
const mockLinkSource = mock();
const mockUnlinkSource = mock();

// Mock projects API
mock.module('../../../api/projects', () => ({
  projectsApi: {
    getProjects: mockGetProjects,
    getSyncConfigs: mockGetSyncConfigs,
    createSyncConfig: mockCreateSyncConfig,
    deleteSyncConfig: mockDeleteSyncConfig,
    setWebhookSecret: mockSetWebhookSecret,
    createProject: mockCreateProject,
    updateProject: mockUpdateProject,
    deleteProject: mockDeleteProject,
    startAutoProject: mock(),
    getAutomation: mock(),
    resumeAutomation: mock(),
  },
}));

// Mock client (for sources)
mock.module('../../../api/client', () => ({
  client: {
    getSources: mockGetSources,
    linkSource: mockLinkSource,
    unlinkSource: mockUnlinkSource,
  },
}));

// Mock useAuth
mock.module('../../auth/context', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    user: { id: '1', email: 'test@test.com' },
    roles: ['user'],
    permissions: ['projects:read', 'projects:create'],
    hasPermission: () => true,
    hasAnyPermission: () => true,
    hasRole: () => true,
    logout: mock(),
    login: mock(),
  }),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

// Mock useWorkspace
mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentWorkspace: { id: 'test-workspace-id', name: 'Test Workspace' },
    currentOrganization: { id: 'test-org-id', name: 'Test Org' },
    workspaces: [],
    organizations: [],
    loading: false,
    error: null,
    setCurrentWorkspace: mock(),
    setCurrentOrganization: mock(),
    refreshWorkspaces: mock(),
    refreshOrganizations: mock(),
  }),
}));

let ProjectsPage: typeof import('./ProjectsPage').default;

beforeAll(async () => {
  ProjectsPage = (await import('./ProjectsPage')).default;
});

afterAll(() => {
  mock.restore();
});

const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false, gcTime: 0 },
    },
  });
  return ({ children }: { children: React.ReactNode }) => (
    <MemoryRouter initialEntries={['/projects']}>
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    </MemoryRouter>
  );
};

const renderWithQueryClient = (ui: React.ReactElement) => {
  const Wrapper = createWrapper();
  return render(<Wrapper>{ui}</Wrapper>);
};

const mockProject: Project = {
  id: 'proj-1',
  name: 'Test Project',
  description: 'Test description',
  status: 'active',
  github_repo_url: null,
  source_id: null,
  auto: false,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

const mockSyncConfigs: SyncConfig[] = [
  {
    id: 'sync-1',
    project_id: 'proj-1',
    provider: 'github',
    direction: 'bidirectional',
    external_repo_url: 'https://github.com/user/repo',
    is_active: true,
    webhook_secret_issued_by_zone: true,
    created_at: '2024-01-01T00:00:00Z',
  },
];

// Note: Tests fail because .closest('.project-card') returns null in bun:test environment
describe('ProjectsPage - Sync Configuration', () => {
  beforeEach(() => {
    mockGetProjects.mockReset();
    mockGetSyncConfigs.mockReset();
    mockGetSources.mockReset();
    mockCreateSyncConfig.mockReset();
    mockSetWebhookSecret.mockReset();
    mockGetProjects.mockResolvedValue([mockProject]);
    mockGetSources.mockResolvedValue([]);
    mockGetSyncConfigs.mockResolvedValue(mockSyncConfigs);
  });

  it('should display sync section when project is selected', async () => {
    renderWithQueryClient(<ProjectsPage />);

    // Wait for projects to load
    await waitFor(() => {
      expect(screen.getByText('Test Project')).toBeInTheDocument();
    });

    // Click on project card
    const projectCard = screen.getByText('Test Project').closest('.project-card');
    fireEvent.click(projectCard!);

    // Wait for sync section to appear
    await waitFor(() => {
      expect(screen.getByText('External Sync')).toBeInTheDocument();
      expect(screen.getByText('+ Add Sync')).toBeInTheDocument();
    });

    // Verify getSyncConfigs was called
    expect(mockGetSyncConfigs).toHaveBeenCalledWith('proj-1');
  });

  it('should show empty state when no sync configs exist', async () => {
    mockGetSyncConfigs.mockResolvedValue([]);

    renderWithQueryClient(<ProjectsPage />);

    await waitFor(() => {
      expect(screen.getByText('Test Project')).toBeInTheDocument();
    });

    const projectCard = screen.getByText('Test Project').closest('.project-card');
    fireEvent.click(projectCard!);

    await waitFor(() => {
      expect(screen.getByText('External Sync')).toBeInTheDocument();
    });

    await waitFor(() => {
      expect(screen.getByText(/No sync configured/)).toBeInTheDocument();
    });
  });

  it('says a configuration is configured but not yet synced, and where its webhook lands', async () => {
    mockGetSyncConfigs.mockResolvedValue([
      {
        ...mockSyncConfigs[0],
        status: 'configured',
        last_synced_at: null,
        webhook_path: '/api/webhooks/sync/sync-1/github',
      },
    ]);

    renderWithQueryClient(<ProjectsPage />);

    await waitFor(() => {
      expect(screen.getByText('Test Project')).toBeInTheDocument();
    });
    fireEvent.click(screen.getByText('Test Project').closest('.project-card')!);

    await waitFor(() => {
      expect(screen.getByText('Configured, not yet synced')).toBeInTheDocument();
    });
    expect(
      screen.getByText(`${window.location.origin}/api/webhooks/sync/sync-1/github`)
    ).toBeInTheDocument();
    expect(screen.queryByText(/Synced /)).toBeNull();
  });

  it('should open add sync modal when clicking add button', async () => {
    renderWithQueryClient(<ProjectsPage />);

    await waitFor(() => {
      expect(screen.getByText('Test Project')).toBeInTheDocument();
    });

    const projectCard = screen.getByText('Test Project').closest('.project-card');
    fireEvent.click(projectCard!);

    await waitFor(() => {
      expect(screen.getByText('External Sync')).toBeInTheDocument();
    });

    const addButton = screen.getByText('+ Add Sync');
    fireEvent.click(addButton);

    await waitFor(() => {
      expect(screen.getByText('Add External Sync')).toBeInTheDocument();
      expect(screen.getByLabelText('Provider')).toBeInTheDocument();
      expect(screen.getByLabelText('Direction')).toBeInTheDocument();
    });
  });

  it('should show GitHub repo URL field by default', async () => {
    renderWithQueryClient(<ProjectsPage />);

    await waitFor(() => {
      expect(screen.getByText('Test Project')).toBeInTheDocument();
    });

    const projectCard = screen.getByText('Test Project').closest('.project-card');
    fireEvent.click(projectCard!);

    await waitFor(() => {
      expect(screen.getByText('External Sync')).toBeInTheDocument();
    });

    const addButton = screen.getByText('+ Add Sync');
    fireEvent.click(addButton);

    await waitFor(() => {
      expect(screen.getByLabelText('Repository URL')).toBeInTheDocument();
    });
  });

  it('should switch to Linear project ID field when provider changes', async () => {
    renderWithQueryClient(<ProjectsPage />);

    await waitFor(() => {
      expect(screen.getByText('Test Project')).toBeInTheDocument();
    });

    const projectCard = screen.getByText('Test Project').closest('.project-card');
    fireEvent.click(projectCard!);

    await waitFor(() => {
      expect(screen.getByText('External Sync')).toBeInTheDocument();
    });

    const addButton = screen.getByText('+ Add Sync');
    fireEvent.click(addButton);

    const providerSelect = screen.getByLabelText('Provider') as HTMLSelectElement;
    fireEvent.change(providerSelect, { target: { value: 'linear' } });

    await waitFor(() => {
      expect(screen.getByLabelText('Project ID')).toBeInTheDocument();
    });
  });

  describe('webhook secrets', () => {
    const secret = 'a'.repeat(64);
    const rotated = 'b'.repeat(64);
    const githubConfig: SyncConfig = {
      ...mockSyncConfigs[0],
      webhook_path: '/api/webhooks/sync/sync-1/github',
      webhook_secret_configured: true,
    };
    const linearConfig: SyncConfig = {
      id: 'sync-2',
      project_id: 'proj-1',
      provider: 'linear',
      direction: 'inbound',
      external_project_id: '2f1c1b8e-7a8d-4a55-9d53-2b5f0e0c9a11',
      is_active: true,
      webhook_secret_issued_by_zone: false,
      created_at: '2024-01-01T00:00:00Z',
      webhook_path: '/api/webhooks/sync/sync-2/linear',
      webhook_secret_configured: false,
    };

    const openProject = async () => {
      renderWithQueryClient(<ProjectsPage />);
      await waitFor(() => {
        expect(screen.getByText('Test Project')).toBeInTheDocument();
      });
      fireEvent.click(screen.getByText('Test Project').closest('.project-card')!);
    };

    const rotateConfirmed = async () => {
      fireEvent.click(await screen.findByRole('button', { name: 'Rotate secret' }));
      fireEvent.click(await screen.findByRole('button', { name: 'Generate new secret' }));
    };

    it('shows the generated secret once after creating a GitHub sync, and not after dismissing or refetching', async () => {
      mockGetSyncConfigs.mockResolvedValue([]);
      mockCreateSyncConfig.mockImplementation(async () => {
        mockGetSyncConfigs.mockResolvedValue([githubConfig]);
        return { config: githubConfig, webhookSecret: secret };
      });

      await openProject();
      await waitFor(() => {
        expect(screen.getByText(/No sync configured/)).toBeInTheDocument();
      });
      fireEvent.click(screen.getByText('+ Add Sync'));
      fireEvent.change(screen.getByLabelText('Repository URL'), {
        target: { value: 'https://github.com/user/repo' },
      });
      fireEvent.click(screen.getByText('Add Sync Config'));

      await waitFor(() => {
        expect(screen.getByTestId('sync-secret-value').textContent).toBe(secret);
      });
      expect(screen.getByText(/content type application\/json/)).toBeInTheDocument();
      expect(screen.getByText(/events: Issues/)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Copy' })).toBeInTheDocument();

      fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
      expect(screen.queryByText(secret)).toBeNull();

      const reads = mockGetSyncConfigs.mock.calls.length;
      fireEvent.click(screen.getByLabelText('Close'));
      fireEvent.click(screen.getByText('Test Project').closest('.project-card')!);
      await waitFor(() => {
        expect(mockGetSyncConfigs.mock.calls.length).toBeGreaterThan(reads);
      });
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Rotate secret' })).toBeInTheDocument();
      });
      expect(screen.queryByText(secret)).toBeNull();
      expect(screen.queryByTestId('sync-secret')).toBeNull();
    });

    it('drops a revealed secret once the project is closed', async () => {
      mockGetSyncConfigs.mockResolvedValue([githubConfig]);
      mockSetWebhookSecret.mockResolvedValue({ config: githubConfig, webhookSecret: rotated });

      await openProject();
      await rotateConfirmed();
      await screen.findByText(rotated);

      fireEvent.click(screen.getByLabelText('Close'));
      fireEvent.click(screen.getByText('Test Project').closest('.project-card')!);
      await screen.findByRole('button', { name: 'Rotate secret' });
      expect(screen.queryByText(rotated)).toBeNull();
    });

    it('rotates a GitHub secret by asking the server to generate one and shows the new secret', async () => {
      mockGetSyncConfigs.mockResolvedValue([githubConfig]);
      mockSetWebhookSecret.mockResolvedValue({ config: githubConfig, webhookSecret: rotated });

      await openProject();
      await rotateConfirmed();

      await waitFor(() => {
        expect(screen.getByTestId('sync-secret-value').textContent).toBe(rotated);
      });
      expect(mockSetWebhookSecret).toHaveBeenCalledWith('proj-1', 'sync-1', undefined);
    });

    it('asks before rotating, warning that the live webhook breaks until the new secret is pasted', async () => {
      mockGetSyncConfigs.mockResolvedValue([githubConfig]);
      mockSetWebhookSecret.mockResolvedValue({ config: githubConfig, webhookSecret: rotated });

      await openProject();
      fireEvent.click(await screen.findByRole('button', { name: 'Rotate secret' }));

      const dialog = await screen.findByRole('dialog', { name: 'Rotate webhook secret?' });
      expect(dialog.textContent).toContain('stops accepting the current one straight away');
      expect(dialog.textContent).toContain(
        'GitHub webhook keeps signing with the old secret, so its deliveries are refused until you paste the new one'
      );
      expect(mockSetWebhookSecret).not.toHaveBeenCalled();

      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
      await waitFor(() => {
        expect(screen.queryByRole('dialog')).toBeNull();
      });
      expect(mockSetWebhookSecret).not.toHaveBeenCalled();
      expect(screen.queryByTestId('sync-secret')).toBeNull();
    });

    it('shows each newly rotated secret in a fresh callout', async () => {
      const again = 'c'.repeat(64);
      mockGetSyncConfigs.mockResolvedValue([githubConfig]);
      mockSetWebhookSecret
        .mockResolvedValueOnce({ config: githubConfig, webhookSecret: rotated })
        .mockResolvedValueOnce({ config: githubConfig, webhookSecret: again });

      await openProject();
      await rotateConfirmed();
      await screen.findByText(rotated);
      fireEvent.click(screen.getByRole('button', { name: 'Copy' }));
      await waitFor(() => {
        expect(screen.queryByRole('button', { name: 'Copy' })).toBeNull();
      });

      await rotateConfirmed();
      await screen.findByText(again);
      expect(screen.queryByText(rotated)).toBeNull();
      expect(screen.getByRole('button', { name: 'Copy' })).toBeInTheDocument();
    });

    it('offers rotation where the server says Zone issues the secret, whatever the provider', async () => {
      mockGetSyncConfigs.mockResolvedValue([
        { ...githubConfig, webhook_secret_issued_by_zone: false },
      ]);

      await openProject();

      await screen.findByLabelText('Set signing secret');
      expect(screen.queryByRole('button', { name: 'Rotate secret' })).toBeNull();
    });

    it('keeps the Linear signing secret out of password managers while masking it', async () => {
      mockGetSyncConfigs.mockResolvedValue([linearConfig]);

      await openProject();
      const input = (await screen.findByLabelText('Set signing secret')) as HTMLInputElement;

      expect(input.type).toBe('text');
      expect(input.classList.contains('sync-secret-input')).toBe(true);
      expect(input.getAttribute('autocomplete')).toBe('off');
      expect(input.hasAttribute('data-1p-ignore')).toBe(true);
      expect(input.getAttribute('data-lpignore')).toBe('true');
      expect(input.getAttribute('data-form-type')).toBe('other');
    });

    it('saves the signing secret Linear issued, without echoing it', async () => {
      mockGetSyncConfigs.mockResolvedValue([linearConfig]);
      mockSetWebhookSecret.mockResolvedValue({
        config: { ...linearConfig, webhook_secret_configured: true },
        webhookSecret: null,
      });
      const signing = 'lin_wh_0123456789abcdef';

      await openProject();
      const input = await screen.findByLabelText('Set signing secret');
      expect(screen.getByText(/Create an Issues webhook in Linear/)).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Rotate secret' })).toBeNull();

      fireEvent.change(input, { target: { value: `  ${signing}  ` } });
      fireEvent.click(screen.getByRole('button', { name: 'Save' }));

      await waitFor(() => {
        expect(mockSetWebhookSecret).toHaveBeenCalledWith('proj-1', 'sync-2', signing);
      });
      await screen.findByText('Signing secret saved');
      expect((input as HTMLInputElement).value).toBe('');
      expect(screen.queryByTestId('sync-secret')).toBeNull();
    });

    it('refuses a Linear signing secret shorter than 16 characters before sending it', async () => {
      mockGetSyncConfigs.mockResolvedValue([linearConfig]);

      await openProject();
      fireEvent.change(await screen.findByLabelText('Set signing secret'), {
        target: { value: 'too-short' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Save' }));

      expect(await screen.findByText(/at least 16 characters/)).toBeInTheDocument();
      expect(mockSetWebhookSecret).not.toHaveBeenCalled();
    });

    it('warns that deliveries are refused while a configuration has no secret', async () => {
      mockGetSyncConfigs.mockResolvedValue([
        { ...githubConfig, webhook_secret_configured: false },
        linearConfig,
      ]);
      mockSetWebhookSecret.mockResolvedValue({ config: githubConfig, webhookSecret: secret });

      await openProject();

      await waitFor(() => {
        expect(
          screen.getAllByText('No webhook secret: deliveries are refused until one is set')
        ).toHaveLength(2);
      });
      expect(screen.queryByRole('button', { name: 'Rotate secret' })).toBeNull();
      expect(screen.getAllByRole('button', { name: 'Generate secret' })).toHaveLength(1);

      fireEvent.click(screen.getByRole('button', { name: 'Generate secret' }));
      await waitFor(() => {
        expect(screen.getByTestId('sync-secret-value').textContent).toBe(secret);
      });
      expect(mockSetWebhookSecret).toHaveBeenCalledWith('proj-1', 'sync-1', undefined);
    });

    it('tells a Linear sync that the project ID is the Linear project UUID', async () => {
      await openProject();
      fireEvent.click(await screen.findByText('+ Add Sync'));
      fireEvent.change(screen.getByLabelText('Provider'), { target: { value: 'linear' } });

      expect(await screen.findByText(/Linear project's ID \(a UUID\)/)).toBeInTheDocument();
    });
  });
});
