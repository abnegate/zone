import { afterAll, beforeAll, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type React from 'react';
import type { Source } from '../../../types';
import type { Project } from '../types';

let ProjectsPage: typeof import('./ProjectsPage').default;

const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false, gcTime: 0 },
    },
  });
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
};

const renderWithQueryClient = (ui: React.ReactElement) => {
  const Wrapper = createWrapper();
  return render(<Wrapper>{ui}</Wrapper>);
};

// Create mock functions for the projects API
const mockGetProjects = mock(() => Promise.resolve([] as Project[]));
const mockCreateProject = mock(() => Promise.resolve({} as Project));
const mockUpdateProject = mock(() => Promise.resolve({} as Project));
const mockDeleteProject = mock(() => Promise.resolve());
const mockGetSyncConfigs = mock(() => Promise.resolve([]));
const mockCreateSyncConfig = mock(() => Promise.resolve({}));
const mockDeleteSyncConfig = mock(() => Promise.resolve());

// Create mock functions for the client
const mockGetSources = mock(() => Promise.resolve([] as Source[]));
const mockLinkSource = mock(() => Promise.resolve({} as Project));
const mockUnlinkSource = mock(() => Promise.resolve({} as Project));

// Mock projects API module
mock.module('../../../api/projects', () => ({
  projectsApi: {
    getProjects: mockGetProjects,
    createProject: mockCreateProject,
    updateProject: mockUpdateProject,
    deleteProject: mockDeleteProject,
    getSyncConfigs: mockGetSyncConfigs,
    createSyncConfig: mockCreateSyncConfig,
    deleteSyncConfig: mockDeleteSyncConfig,
  },
}));

// Mock client module
mock.module('../../../api/client', () => ({
  client: {
    getSources: mockGetSources,
    linkSource: mockLinkSource,
    unlinkSource: mockUnlinkSource,
  },
}));

// Mock useAuth
mock.module('../../../features/auth', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    user: { id: '1', email: 'test@test.com' },
    roles: ['user'],
    permissions: ['projects:read', 'projects:create', 'projects:update', 'projects:delete'],
    hasPermission: () => true,
    hasAnyPermission: () => true,
    hasRole: () => true,
    logout: mock(() => {}),
    login: mock(() => {}),
    register: mock(() => {}),
    setAccessToken: mock(() => {}),
  }),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
  ResendVerificationButton: () => null,
  VerificationPendingBanner: () => null,
}));

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentWorkspace: {
      id: 'workspace-1',
      name: 'Test Workspace',
    },
    loading: false,
  }),
}));

beforeAll(async () => {
  ProjectsPage = (await import('./ProjectsPage')).default;
});

afterAll(() => {
  mock.restore();
});

const mockProjects: Project[] = [
  {
    id: 'proj-1',
    name: 'Project Alpha',
    description: 'First project',
    status: 'active',
    github_repo_url: null,
    source_id: 'src-1',
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-15T00:00:00Z',
  },
  {
    id: 'proj-2',
    name: 'Project Beta',
    description: 'Second project',
    status: 'on_hold',
    github_repo_url: null,
    source_id: null,
    created_at: '2024-01-02T00:00:00Z',
    updated_at: '2024-01-16T00:00:00Z',
  },
];

const mockSources: Source[] = [
  {
    id: 'src-1',
    name: 'GitHub Repo',
    source_type: 'github',
    category: 'file',
    config: { owner: 'test', repo: 'repo' },
    description: null,
    url: 'https://github.com/test/repo',
    is_active: true,
    last_verified_at: null,
    last_error: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
  },
];

describe('ProjectsPage', () => {
  beforeEach(() => {
    mockGetProjects.mockReset();
    mockCreateProject.mockReset();
    mockUpdateProject.mockReset();
    mockDeleteProject.mockReset();
    mockGetSyncConfigs.mockReset();
    mockGetSources.mockReset();
    mockLinkSource.mockReset();
    mockUnlinkSource.mockReset();
    mockGetProjects.mockImplementation(() => Promise.resolve(mockProjects));
    mockGetSyncConfigs.mockImplementation(() => Promise.resolve([]));
    mockGetSources.mockImplementation(() => Promise.resolve(mockSources));
  });

  it('shows loading state', async () => {
    mockGetProjects.mockImplementation(() => new Promise(() => {}));
    renderWithQueryClient(<ProjectsPage />);
    expect(screen.getByText('Loading projects...')).toBeInTheDocument();
  });

  it('shows error state', async () => {
    mockGetProjects.mockImplementation(() => Promise.reject(new Error('Failed to load')));
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Failed to load')).toBeInTheDocument();
    });
  });

  it('shows empty state', async () => {
    mockGetProjects.mockImplementation(() => Promise.resolve([]));
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('No projects yet')).toBeInTheDocument();
    });
  });

  it('renders projects list', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
      expect(screen.getByText('Project Beta')).toBeInTheDocument();
    });
  });

  it('renders page header', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Projects' })).toBeInTheDocument();
    });
    expect(screen.getByText('Organize work with GitHub integration')).toBeInTheDocument();
  });

  // Note: Filters use TabsTrigger which has role="tab"
  it('renders filter buttons', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'All' })).toBeInTheDocument();
    });
    expect(screen.getByRole('tab', { name: 'Active' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'On Hold' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Cancelled' })).toBeInTheDocument();
  });

  // Note: Filters use TabsTrigger which has role="tab"
  it('filters projects by status', async () => {
    const user = userEvent.setup();
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Active' }));
    await waitFor(() => {
      expect(mockGetProjects).toHaveBeenCalledWith('workspace-1', 'active');
    });
  });

  it('opens create project wizard', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Project' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Project' }));
    expect(screen.getByRole('heading', { name: 'New Project' })).toBeInTheDocument();
  });

  // Note: Wizard step progression doesn't work correctly in test environment
  it('creates new project via wizard', async () => {
    const newProject: Project = {
      id: 'proj-3',
      name: 'New Project',
      description: 'New description',
      status: 'active',
      github_repo_url: null,
      source_id: null,
      created_at: '2024-01-17T00:00:00Z',
      updated_at: '2024-01-17T00:00:00Z',
    };
    mockCreateProject.mockImplementation(() => Promise.resolve(newProject));

    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Project' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Project' }));

    // Step 1: Project details
    fireEvent.change(screen.getByLabelText('Project Name'), { target: { value: 'New Project' } });
    fireEvent.change(screen.getByLabelText(/Description/), {
      target: { value: 'New description' },
    });

    // Go to step 2 (source selection)
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    // Step 2: Skip source (select "No Source")
    await waitFor(() => {
      expect(screen.getByText('No Source')).toBeInTheDocument();
    });

    // Go to step 3 (status)
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    // Step 3: Status is active by default, complete the wizard
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Create Project' })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Create Project' }));

    await waitFor(() => {
      expect(mockCreateProject).toHaveBeenCalledWith({
        name: 'New Project',
        description: 'New description',
        status: 'active',
        source_id: undefined,
        workspace_id: 'workspace-1',
      });
    });
  });

  it('selects a project to view details', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Project Alpha'));

    await waitFor(() => {
      expect(document.querySelector('.project-details')).toBeInTheDocument();
    });
    // Check details panel has expected elements
    expect(screen.getByRole('button', { name: 'Edit Project' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
  });

  it('opens edit project modal', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Project Alpha'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Edit Project' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Edit Project' }));
    expect(screen.getByRole('heading', { name: 'Edit Project' })).toBeInTheDocument();
  });

  it('updates project', async () => {
    const updatedProject: Project = {
      ...mockProjects[0],
      name: 'Updated Alpha',
    };
    mockUpdateProject.mockImplementation(() => Promise.resolve(updatedProject));

    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Project Alpha'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Edit Project' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Edit Project' }));
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Updated Alpha' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));

    await waitFor(() => {
      expect(mockUpdateProject).toHaveBeenCalled();
    });
  });

  it('shows delete confirmation', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Project Alpha'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(screen.getByRole('heading', { name: 'Delete Project' })).toBeInTheDocument();
  });

  it('deletes project', async () => {
    mockDeleteProject.mockImplementation(() => Promise.resolve(undefined));

    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Project Alpha'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete Project' }));

    await waitFor(() => {
      expect(mockDeleteProject).toHaveBeenCalledWith('proj-1');
    });
  });

  it('closes project details', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Project Alpha'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Close' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(document.querySelector('.project-details')).not.toBeInTheDocument();
    });
  });

  it('displays project status badges', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Active')).toBeInTheDocument();
      expect(screen.getByText('On Hold')).toBeInTheDocument();
    });
  });

  it('displays project source link', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repo')).toBeInTheDocument();
    });
  });

  it('displays no source message for projects without source', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('No source')).toBeInTheDocument();
    });
  });

  it('cancels create wizard', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Project' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Project' }));
    expect(screen.getByRole('heading', { name: 'New Project' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: 'New Project' })).not.toBeInTheDocument();
    });
  });

  it('handles keyboard navigation on project card', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    const projectCard = screen.getByText('Project Alpha').closest('[role="button"]')!;
    fireEvent.keyDown(projectCard, { key: 'Enter' });

    await waitFor(() => {
      expect(document.querySelector('.project-details')).toBeInTheDocument();
    });
  });

  it('shows link source button when no source linked', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Beta')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Project Beta'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Link Source' })).toBeInTheDocument();
    });
  });

  it('opens link source modal', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Beta')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Project Beta'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Link Source' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Link Source' }));
    expect(screen.getByRole('heading', { name: 'Link Source' })).toBeInTheDocument();
  });

  it('links source to project', async () => {
    const updatedProject: Project = {
      ...mockProjects[1],
      source_id: 'src-1',
    };
    mockLinkSource.mockImplementation(() => Promise.resolve(updatedProject));

    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Beta')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Project Beta'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Link Source' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Link Source' }));
    fireEvent.change(screen.getByLabelText('Source'), { target: { value: 'src-1' } });

    const linkButtons = screen.getAllByRole('button', { name: /Link Source/i });
    fireEvent.click(linkButtons[linkButtons.length - 1]); // Click the submit button

    await waitFor(() => {
      expect(mockLinkSource).toHaveBeenCalledWith('proj-2', 'src-1');
    });
  });

  it('shows unlink button for project with source', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Project Alpha'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Unlink' })).toBeInTheDocument();
    });
  });

  it('unlinks source from project', async () => {
    const updatedProject: Project = {
      ...mockProjects[0],
      source_id: null,
    };
    mockUnlinkSource.mockImplementation(() => Promise.resolve(updatedProject));

    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Project Alpha'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Unlink' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Unlink' }));

    await waitFor(() => {
      expect(mockUnlinkSource).toHaveBeenCalledWith('proj-1');
    });
  });

  it('handles create project error', async () => {
    mockCreateProject.mockImplementation(() => Promise.reject(new Error('Create failed')));

    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Project' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Project' }));
    fireEvent.change(screen.getByLabelText('Project Name'), { target: { value: 'New Project' } });

    // Navigate through wizard steps
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));
    await waitFor(() => {
      expect(screen.getByText('No Source')).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Create Project' })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Create Project' }));

    await waitFor(() => {
      expect(screen.getByText('Create failed')).toBeInTheDocument();
    });
  });

  it('handles update project error', async () => {
    const consoleErrorSpy = spyOn(console, 'error').mockImplementation();
    mockUpdateProject.mockImplementation(() => Promise.reject(new Error('Update failed')));

    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Project Alpha'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Edit Project' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Edit Project' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));

    await waitFor(() => {
      expect(consoleErrorSpy).toHaveBeenCalledWith('Failed to update project:', expect.any(Error));
    });

    consoleErrorSpy.mockRestore();
  });

  it('handles delete project error', async () => {
    const consoleErrorSpy = spyOn(console, 'error').mockImplementation();
    mockDeleteProject.mockImplementation(() => Promise.reject(new Error('Delete failed')));

    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Project Alpha'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete Project' }));

    await waitFor(() => {
      expect(consoleErrorSpy).toHaveBeenCalledWith('Failed to delete project:', expect.any(Error));
    });

    consoleErrorSpy.mockRestore();
  });

  it('can create project from empty state', async () => {
    mockGetProjects.mockImplementation(() => Promise.resolve([]));

    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('No projects yet')).toBeInTheDocument();
    });

    // The empty state button opens the wizard
    const createButtons = screen.getAllByRole('button', { name: /Create Project|New Project/i });
    fireEvent.click(createButtons[0]);
    expect(screen.getByRole('heading', { name: 'New Project' })).toBeInTheDocument();
  });

  // Note: Filters use TabsTrigger which has role="tab" not role="button"
  it('filters by on_hold status', async () => {
    const user = userEvent.setup();
    const projectsWithOnHold: Project[] = [
      ...mockProjects,
      {
        id: 'proj-3',
        name: 'On Hold Project',
        description: 'Paused',
        status: 'on_hold',
        github_repo_url: null,
        source_id: null,
        created_at: '2024-01-03T00:00:00Z',
        updated_at: '2024-01-03T00:00:00Z',
      },
    ];
    mockGetProjects.mockImplementation(() => Promise.resolve(projectsWithOnHold));

    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('On Hold Project')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'On Hold' }));

    await waitFor(() => {
      expect(mockGetProjects).toHaveBeenCalledWith('workspace-1', 'on_hold');
    });
  });

  // Note: Filters use TabsTrigger which has role="tab" not role="button"
  it('filters by cancelled status', async () => {
    const user = userEvent.setup();
    const projectsWithCancelled: Project[] = [
      {
        id: 'proj-4',
        name: 'Cancelled Project',
        description: 'No longer needed',
        status: 'cancelled',
        github_repo_url: null,
        source_id: null,
        created_at: '2024-01-04T00:00:00Z',
        updated_at: '2024-01-04T00:00:00Z',
      },
    ];
    let callCount = 0;
    mockGetProjects.mockImplementation(() => {
      callCount++;
      return Promise.resolve(callCount === 1 ? mockProjects : projectsWithCancelled);
    });

    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Cancelled' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Cancelled' }));

    await waitFor(() => {
      expect(mockGetProjects).toHaveBeenCalledWith('workspace-1', 'cancelled');
    });
  });

  it('selects project via keyboard', async () => {
    renderWithQueryClient(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    const projectCard = screen.getByText('Project Alpha').closest('[role="button"]');
    fireEvent.keyDown(projectCard!, { key: 'Enter' });

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Project Alpha', level: 2 })).toBeInTheDocument();
    });
  });
});
