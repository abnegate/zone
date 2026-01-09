import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { client } from '../../../api/client';
import { projectsApi } from '../../../api/projects';
import type { Project, SyncConfig } from '../types';
import ProjectsPage from './ProjectsPage';

// Mock projects API
jest.mock('../../../api/projects', () => ({
  projectsApi: {
    getProjects: jest.fn(),
    getSyncConfigs: jest.fn(),
    createSyncConfig: jest.fn(),
    deleteSyncConfig: jest.fn(),
    createProject: jest.fn(),
    updateProject: jest.fn(),
    deleteProject: jest.fn(),
  },
}));

// Mock client (for sources)
jest.mock('../../../api/client', () => ({
  client: {
    getSources: jest.fn(),
    linkSource: jest.fn(),
    unlinkSource: jest.fn(),
  },
}));

// Mock useAuth
jest.mock('../../auth/context', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    user: { id: '1', email: 'test@test.com' },
    roles: ['user'],
    permissions: ['projects:read', 'projects:create'],
    hasPermission: () => true,
    hasAnyPermission: () => true,
    hasRole: () => true,
    logout: jest.fn(),
    login: jest.fn(),
  }),
}));

const mockProjectsApi = projectsApi as jest.Mocked<typeof projectsApi>;
const mockClient = client as jest.Mocked<typeof client>;

const mockProject: Project = {
  id: 'proj-1',
  name: 'Test Project',
  description: 'Test description',
  status: 'active',
  github_repo_url: null,
  source_id: null,
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
    created_at: '2024-01-01T00:00:00Z',
  },
];

describe('ProjectsPage - Sync Configuration', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    mockProjectsApi.getProjects.mockResolvedValue([mockProject]);
    mockClient.getSources.mockResolvedValue([]);
    mockProjectsApi.getSyncConfigs.mockResolvedValue(mockSyncConfigs);
  });

  it('should display sync section when project is selected', async () => {
    render(<ProjectsPage />);

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
    expect(mockProjectsApi.getSyncConfigs).toHaveBeenCalledWith('proj-1');
  });

  it('should show empty state when no sync configs exist', async () => {
    mockProjectsApi.getSyncConfigs.mockResolvedValue([]);

    render(<ProjectsPage />);

    await waitFor(() => {
      expect(screen.getByText('Test Project')).toBeInTheDocument();
    });

    const projectCard = screen.getByText('Test Project').closest('.project-card');
    fireEvent.click(projectCard!);

    await waitFor(() => {
      expect(screen.getByText('External Sync')).toBeInTheDocument();
    });

    await waitFor(() => {
      expect(screen.getByText(/No sync configurations/)).toBeInTheDocument();
    });
  });

  it('should open add sync modal when clicking add button', async () => {
    render(<ProjectsPage />);

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
    render(<ProjectsPage />);

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
    render(<ProjectsPage />);

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
});
