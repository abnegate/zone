import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type React from 'react';
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from 'react-router-dom';
import type { Source } from '../../../types';
import type { Project, ProjectAutomation } from '../types';

let ProjectsPage: typeof import('./ProjectsPage').default;

/** Where the page navigated to, and a way to follow a project link again. */
const LocationProbe = () => {
  const location = useLocation();
  const navigate = useNavigate();
  return (
    <div>
      <div data-testid="location">{`${location.pathname}${location.search}`}</div>
      <button
        type="button"
        data-testid="follow-proj-2"
        onClick={() => navigate('/projects?id=proj-2')}
      >
        follow
      </button>
    </div>
  );
};

const createWrapper = (initialPath: string) => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false, gcTime: 0 },
    },
  });
  return ({ children }: { children: React.ReactNode }) => (
    <MemoryRouter initialEntries={[initialPath]}>
      <QueryClientProvider client={queryClient}>
        <Routes>
          <Route
            path="/projects"
            element={
              <>
                <LocationProbe />
                {children}
              </>
            }
          />
          <Route path="*" element={<LocationProbe />} />
        </Routes>
      </QueryClientProvider>
    </MemoryRouter>
  );
};

const renderWithQueryClient = (ui: React.ReactElement, initialPath = '/projects') => {
  const Wrapper = createWrapper(initialPath);
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
const mockStartAutoProject = mock(() => Promise.resolve({ chat_id: 'chat-9' }));
const mockGetAutomation = mock(() => Promise.resolve({} as ProjectAutomation));
const mockResumeAutomation = mock(() => Promise.resolve({} as Project));

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
    startAutoProject: mockStartAutoProject,
    getAutomation: mockGetAutomation,
    resumeAutomation: mockResumeAutomation,
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
    auto: false,
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
    auto: false,
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
    mockStartAutoProject.mockReset();
    mockGetAutomation.mockReset();
    mockResumeAutomation.mockReset();
    mockStartAutoProject.mockImplementation(() => Promise.resolve({ chat_id: 'chat-9' }));
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
      auto: false,
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

  it('shows the server error in the edit modal and keeps it open', async () => {
    mockUpdateProject.mockImplementation(() =>
      Promise.reject(new Error('Workspace write access required'))
    );

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
      expect(screen.getByRole('alert')).toHaveTextContent('Workspace write access required');
    });
    expect(screen.getByRole('heading', { name: 'Edit Project' })).toBeInTheDocument();
  });

  it('shows the server error in the delete dialog and keeps it open', async () => {
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
      expect(screen.getByRole('alert')).toHaveTextContent('Delete failed');
    });
    expect(screen.getByRole('heading', { name: 'Delete Project' })).toBeInTheDocument();
  });

  it('shows an unlink failure beside the project instead of swallowing it', async () => {
    mockUnlinkSource.mockImplementation(() => Promise.reject(new Error('Project not found')));

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
      expect(screen.getByRole('alert')).toHaveTextContent('Project not found');
    });
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
        auto: false,
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
        auto: false,
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

  describe('auto projects', () => {
    const automation: ProjectAutomation = {
      project_id: 'proj-1',
      auto: true,
      actor_id: 'user-1',
      paused_reason: null,
      completed_at: null,
      parallelism: 3,
      planner_chat_id: 'chat-plan',
      updates_chat_id: 'chat-updates',
      counts: { total: 2, agentic: 2, complete: 1, in_flight: 1, paused: 0 },
      tasks: [
        {
          task_id: 'task-1',
          title: 'Scaffold the repository',
          status: 'complete',
          is_agentic: true,
          kind: 'scaffold',
          stage: 'merged',
          reason: null,
          runs: 1,
          review_rounds: 1,
          reviewers: 'reviewer-model',
          pr_url: 'https://github.com/acme/app/pull/1',
          head: 'abc',
          checks: 'success',
          merge_sha: 'def',
          auto_created: false,
        },
        {
          task_id: 'task-2',
          title: 'Add continuous integration',
          status: 'in_progress',
          is_agentic: true,
          kind: 'ci',
          stage: 'running',
          reason: null,
          runs: 1,
          review_rounds: 0,
          reviewers: null,
          pr_url: null,
          head: null,
          checks: null,
          merge_sha: null,
          auto_created: false,
        },
      ],
    };

    it('starts the interview from a brief and opens the planner chat', async () => {
      renderWithQueryClient(<ProjectsPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Auto project' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Auto project' }));
      expect(screen.getByTestId('auto-project-modal')).toBeInTheDocument();

      fireEvent.change(screen.getByTestId('auto-project-brief'), {
        target: { value: 'A recipe app for iOS and Android' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Start the interview' }));

      await waitFor(() => {
        expect(mockStartAutoProject).toHaveBeenCalledWith('workspace-1', {
          brief: 'A recipe app for iOS and Android',
        });
      });
      await waitFor(
        () => {
          expect(screen.getByTestId('location')).toHaveTextContent('/chats?id=chat-9');
        },
        { timeout: 3000 }
      );
      expect(screen.queryByTestId('auto-project-modal')).not.toBeInTheDocument();
    });

    it('shows the automation error instead of leaving the modal', async () => {
      mockStartAutoProject.mockImplementation(() =>
        Promise.reject(new Error('Choose a model first'))
      );
      renderWithQueryClient(<ProjectsPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Auto project' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Auto project' }));
      fireEvent.change(screen.getByTestId('auto-project-brief'), {
        target: { value: 'Something' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Start the interview' }));

      await waitFor(() => {
        expect(screen.getByText('Choose a model first')).toBeInTheDocument();
      });
      expect(screen.getByTestId('auto-project-modal')).toBeInTheDocument();
      expect(screen.getByTestId('location')).toHaveTextContent('/projects');
    });

    it('toggles automation on the selected project', async () => {
      mockUpdateProject.mockImplementation(() =>
        Promise.resolve({ ...mockProjects[0], auto: true })
      );
      mockGetAutomation.mockImplementation(() => Promise.resolve(automation));

      renderWithQueryClient(<ProjectsPage />);
      await waitFor(() => {
        expect(screen.getByText('Project Alpha')).toBeInTheDocument();
      });
      fireEvent.click(screen.getByText('Project Alpha'));

      const toggle = await screen.findByTestId('auto-toggle');
      expect(toggle).toHaveAttribute('aria-pressed', 'false');
      expect(screen.queryByTestId('automation-panel')).not.toBeInTheDocument();

      fireEvent.click(toggle);

      await waitFor(() => {
        expect(mockUpdateProject).toHaveBeenCalledWith('proj-1', { auto: true });
      });
      await waitFor(() => {
        expect(screen.getByTestId('auto-toggle')).toHaveAttribute('aria-pressed', 'true');
      });
      await waitFor(() => {
        expect(mockGetAutomation).toHaveBeenCalledWith('proj-1');
      });
      await waitFor(() => {
        expect(screen.getByText('1 of 2 tasks merged, 1 in flight')).toBeInTheDocument();
      });
      expect(screen.getByText('Scaffold the repository')).toBeInTheDocument();
      expect(screen.getByText('Reviewed by reviewer-model')).toBeInTheDocument();
      expect(screen.getByRole('link', { name: 'Interview' })).toHaveAttribute(
        'href',
        '/chats?id=chat-plan'
      );
    });

    it('marks cards of projects that run themselves and resumes a paused one', async () => {
      const paused = {
        ...automation,
        paused_reason: 'The project token cannot bypass branch protection',
        counts: { ...automation.counts, paused: 1 },
      };
      mockGetProjects.mockImplementation(() =>
        Promise.resolve([{ ...mockProjects[0], auto: true }, mockProjects[1]])
      );
      mockGetAutomation.mockImplementation(() => Promise.resolve(paused));
      mockResumeAutomation.mockImplementation(() =>
        Promise.resolve({ ...mockProjects[0], auto: true })
      );

      renderWithQueryClient(<ProjectsPage />);
      await waitFor(() => {
        expect(screen.getByTestId('auto-badge')).toBeInTheDocument();
      });
      fireEvent.click(screen.getByText('Project Alpha'));

      await waitFor(() => {
        expect(screen.getByTestId('automation-paused')).toBeInTheDocument();
      });
      expect(
        screen.getByText('The project token cannot bypass branch protection')
      ).toBeInTheDocument();

      fireEvent.click(screen.getByRole('button', { name: 'Resume' }));
      await waitFor(() => {
        expect(mockResumeAutomation).toHaveBeenCalledWith('proj-1');
      });
    });

    it('selects the project named in the URL', async () => {
      renderWithQueryClient(<ProjectsPage />, '/projects?id=proj-2');
      await waitFor(
        () => {
          expect(
            screen.getByRole('heading', { name: 'Project Beta', level: 2 })
          ).toBeInTheDocument();
        },
        { timeout: 3000 }
      );
    });

    it('closing the panel consumes the deep link instead of reopening it', async () => {
      renderWithQueryClient(<ProjectsPage />, '/projects?id=proj-2');
      await waitFor(
        () => {
          expect(
            screen.getByRole('heading', { name: 'Project Beta', level: 2 })
          ).toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      fireEvent.click(screen.getByRole('button', { name: 'Close' }));

      await waitFor(() => {
        expect(document.querySelector('.project-details')).not.toBeInTheDocument();
      });
      expect(screen.getByTestId('location')).toHaveTextContent('/projects');
      expect(screen.getByTestId('location')).not.toHaveTextContent('id=');
      // The effect that honours the link must not bring the panel back
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(document.querySelector('.project-details')).not.toBeInTheDocument();

      // Following the same link again, later in the same mount, is honoured
      fireEvent.click(screen.getByTestId('follow-proj-2'));
      await waitFor(
        () => {
          expect(
            screen.getByRole('heading', { name: 'Project Beta', level: 2 })
          ).toBeInTheDocument();
        },
        { timeout: 3000 }
      );
    });

    it('shows why a toggle failed next to the control', async () => {
      mockUpdateProject.mockImplementation(() =>
        Promise.reject(new Error('Automation is disabled on this server'))
      );

      renderWithQueryClient(<ProjectsPage />);
      await waitFor(() => {
        expect(screen.getByText('Project Alpha')).toBeInTheDocument();
      });
      fireEvent.click(screen.getByText('Project Alpha'));
      fireEvent.click(await screen.findByTestId('auto-toggle'));

      const alert = await screen.findByRole('alert');
      expect(alert).toHaveTextContent('Automation is disabled on this server');
      expect(screen.getByTestId('auto-toggle')).toHaveAttribute('aria-pressed', 'false');

      // Choosing another project clears a message that was about this one
      fireEvent.click(screen.getByText('Project Beta'));
      await waitFor(() => {
        expect(screen.queryByRole('alert')).not.toBeInTheDocument();
      });
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
