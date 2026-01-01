import { render, screen, fireEvent, waitFor, act } from '@testing-library/react';
import ProjectsPage from './ProjectsPage';
import { client } from '../api/client';
import type { Project, Source } from '../types';

// Mock client
jest.mock('../api/client', () => ({
  client: {
    getProjects: jest.fn(),
    getSources: jest.fn(),
    createProject: jest.fn(),
    updateProject: jest.fn(),
    deleteProject: jest.fn(),
    linkSource: jest.fn(),
    unlinkSource: jest.fn(),
  },
}));

// Mock useAuth
jest.mock('../context/AuthContext', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    user: { id: '1', email: 'test@test.com' },
    roles: ['user'],
    permissions: ['projects:read', 'projects:create', 'projects:update', 'projects:delete'],
    hasPermission: () => true,
    hasAnyPermission: () => true,
    hasRole: () => true,
    logout: jest.fn(),
    login: jest.fn(),
    register: jest.fn(),
    setAccessToken: jest.fn(),
  }),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

const mockClient = client as jest.Mocked<typeof client>;

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
    jest.clearAllMocks();
    mockClient.getProjects.mockResolvedValue(mockProjects);
    mockClient.getSources.mockResolvedValue(mockSources);
  });

  it('shows loading state', async () => {
    mockClient.getProjects.mockImplementation(() => new Promise(() => {}));
    render(<ProjectsPage />);
    expect(screen.getByText('Loading projects...')).toBeInTheDocument();
  });

  it('shows error state', async () => {
    mockClient.getProjects.mockRejectedValueOnce(new Error('Failed to load'));
    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Failed to load')).toBeInTheDocument();
    });
  });

  it('shows empty state', async () => {
    mockClient.getProjects.mockResolvedValueOnce([]);
    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('No projects yet')).toBeInTheDocument();
    });
  });

  it('renders projects list', async () => {
    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
      expect(screen.getByText('Project Beta')).toBeInTheDocument();
    });
  });

  it('renders page header', async () => {
    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Projects' })).toBeInTheDocument();
    });
    expect(screen.getByText('Organize work with GitHub integration')).toBeInTheDocument();
  });

  it('renders filter buttons', async () => {
    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'All' })).toBeInTheDocument();
    });
    expect(screen.getByRole('button', { name: 'Active' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'On Hold' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Cancelled' })).toBeInTheDocument();
  });

  it('filters projects by status', async () => {
    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Active' }));
    await waitFor(() => {
      expect(mockClient.getProjects).toHaveBeenCalledWith('active');
    });
  });

  it('opens create project modal', async () => {
    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Project' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Project' }));
    expect(screen.getByRole('heading', { name: 'New Project' })).toBeInTheDocument();
  });

  it('creates new project', async () => {
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
    mockClient.createProject.mockResolvedValueOnce(newProject);

    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Project' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Project' }));

    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'New Project' } });
    fireEvent.change(screen.getByLabelText('Description'), { target: { value: 'New description' } });

    fireEvent.click(screen.getByRole('button', { name: 'Create Project' }));

    await waitFor(() => {
      expect(mockClient.createProject).toHaveBeenCalledWith({
        name: 'New Project',
        description: 'New description',
        status: 'active',
        source_id: undefined,
      });
    });
  });

  it('selects a project to view details', async () => {
    render(<ProjectsPage />);
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
    render(<ProjectsPage />);
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
    mockClient.updateProject.mockResolvedValueOnce(updatedProject);

    render(<ProjectsPage />);
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
      expect(mockClient.updateProject).toHaveBeenCalled();
    });
  });

  it('shows delete confirmation', async () => {
    render(<ProjectsPage />);
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
    mockClient.deleteProject.mockResolvedValueOnce(undefined);

    render(<ProjectsPage />);
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
      expect(mockClient.deleteProject).toHaveBeenCalledWith('proj-1');
    });
  });

  it('closes project details', async () => {
    render(<ProjectsPage />);
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
    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Active')).toBeInTheDocument();
      expect(screen.getByText('On Hold')).toBeInTheDocument();
    });
  });

  it('displays project source link', async () => {
    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repo')).toBeInTheDocument();
    });
  });

  it('displays no source message for projects without source', async () => {
    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('No source')).toBeInTheDocument();
    });
  });

  it('cancels create modal', async () => {
    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Project' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Project' }));
    expect(screen.getByRole('heading', { name: 'New Project' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('heading', { name: 'New Project' })).not.toBeInTheDocument();
  });

  it('handles keyboard navigation on project card', async () => {
    render(<ProjectsPage />);
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
    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Beta')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Project Beta'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Link Source' })).toBeInTheDocument();
    });
  });

  it('opens link source modal', async () => {
    render(<ProjectsPage />);
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
    mockClient.linkSource.mockResolvedValueOnce(updatedProject);

    render(<ProjectsPage />);
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
      expect(mockClient.linkSource).toHaveBeenCalledWith('proj-2', 'src-1');
    });
  });

  it('shows unlink button for project with source', async () => {
    render(<ProjectsPage />);
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
    mockClient.unlinkSource.mockResolvedValueOnce(updatedProject);

    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Project Alpha'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Unlink' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Unlink' }));

    await waitFor(() => {
      expect(mockClient.unlinkSource).toHaveBeenCalledWith('proj-1');
    });
  });

  it('handles create project error', async () => {
    mockClient.createProject.mockRejectedValueOnce(new Error('Create failed'));

    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Project' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Project' }));
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'New Project' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create Project' }));

    await waitFor(() => {
      expect(screen.getByText('Create failed')).toBeInTheDocument();
    });
  });

  it('handles update project error', async () => {
    mockClient.updateProject.mockRejectedValueOnce(new Error('Update failed'));

    render(<ProjectsPage />);
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
      expect(screen.getByText('Update failed')).toBeInTheDocument();
    });
  });

  it('handles delete project error', async () => {
    mockClient.deleteProject.mockRejectedValueOnce(new Error('Delete failed'));

    render(<ProjectsPage />);
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
      expect(screen.getByText('Delete failed')).toBeInTheDocument();
    });
  });

  it('can create project from empty state', async () => {
    mockClient.getProjects.mockResolvedValueOnce([]);

    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('No projects yet')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Create Project' }));
    expect(screen.getByRole('heading', { name: 'New Project' })).toBeInTheDocument();
  });

  it('filters by on_hold status', async () => {
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
    mockClient.getProjects.mockResolvedValueOnce(projectsWithOnHold);

    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByText('On Hold Project')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'On Hold' }));

    await waitFor(() => {
      expect(mockClient.getProjects).toHaveBeenCalledWith('on_hold');
    });
  });

  it('filters by cancelled status', async () => {
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
    mockClient.getProjects.mockResolvedValueOnce(mockProjects).mockResolvedValueOnce(projectsWithCancelled);

    render(<ProjectsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Cancelled' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Cancelled' }));

    await waitFor(() => {
      expect(mockClient.getProjects).toHaveBeenCalledWith('cancelled');
    });
  });


  it('selects project via keyboard', async () => {
    render(<ProjectsPage />);
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
