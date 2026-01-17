import { renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import type { Project, CreateProjectRequest, UpdateProjectRequest } from '../types';
import { createElement } from 'react';
import type { ReactNode } from 'react';

const mockGetProjects = mock();
const mockCreateProject = mock();
const mockUpdateProject = mock();
const mockDeleteProject = mock();

mock.module('../../../api/projects', () => ({
  projectsApi: {
    getProjects: mockGetProjects,
    createProject: mockCreateProject,
    updateProject: mockUpdateProject,
    deleteProject: mockDeleteProject,
  },
}));

// Mock useWorkspace to provide a test workspace
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

let useProjects: typeof import('./useProjects').useProjects;

beforeAll(async () => {
  ({ useProjects } = await import('./useProjects'));
});

afterAll(() => {
  mock.restore();
});

// Create a wrapper with QueryClientProvider for testing hooks
const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
};

const mockProjects: Project[] = [
  {
    id: '1',
    name: 'Test Project 1',
    description: 'Description 1',
    status: 'active',
    github_repo_url: null,
    source_id: 'src-1',
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-02T00:00:00Z',
  },
  {
    id: '2',
    name: 'Test Project 2',
    description: null,
    status: 'on_hold',
    github_repo_url: null,
    source_id: null,
    created_at: '2024-01-03T00:00:00Z',
    updated_at: '2024-01-04T00:00:00Z',
  },
];

describe('useProjects', () => {
  beforeEach(() => {
    mockGetProjects.mockReset();
    mockCreateProject.mockReset();
    mockUpdateProject.mockReset();
    mockDeleteProject.mockReset();
  });

  it('should fetch projects on mount', async () => {
    mockGetProjects.mockResolvedValue(mockProjects);

    const { result } = renderHook(() => useProjects(), { wrapper: createWrapper() });

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.projects).toEqual(mockProjects);
    expect(result.current.error).toBeNull();
    expect(mockGetProjects).toHaveBeenCalledWith('test-workspace-id', undefined);
  });

  it('should fetch projects with status filter', async () => {
    mockGetProjects.mockResolvedValue([mockProjects[0]]);

    const { result } = renderHook(() => useProjects('active'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.projects).toEqual([mockProjects[0]]);
    expect(mockGetProjects).toHaveBeenCalledWith('test-workspace-id', 'active');
  });

  it('should handle fetch error', async () => {
    const error = new Error('Failed to fetch');
    mockGetProjects.mockRejectedValue(error);

    const { result } = renderHook(() => useProjects(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.projects).toEqual([]);
    expect(result.current.error).toBe('Failed to fetch');
  });

  it('should create project', async () => {
    const newProject: Project = {
      id: '3',
      name: 'New Project',
      description: 'New Description',
      status: 'active',
      github_repo_url: null,
      source_id: null,
      created_at: '2024-01-05T00:00:00Z',
      updated_at: '2024-01-05T00:00:00Z',
    };

    // First call returns initial projects, second call (after refetch) returns updated list
    mockGetProjects
      .mockResolvedValueOnce(mockProjects)
      .mockResolvedValueOnce([...mockProjects, newProject]);
    mockCreateProject.mockResolvedValue(newProject);

    const { result } = renderHook(() => useProjects(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const createRequest: CreateProjectRequest = {
      name: 'New Project',
      description: 'New Description',
      status: 'active',
    };

    await result.current.createProject(createRequest);

    await waitFor(() => {
      expect(result.current.projects).toContainEqual(newProject);
    });

    expect(mockCreateProject).toHaveBeenCalledWith(createRequest);
  });

  it('should update project', async () => {
    const updatedProject: Project = {
      ...mockProjects[0],
      name: 'Updated Name',
      status: 'on_hold',
    };

    // First call returns initial projects, second call (after refetch) returns updated list
    mockGetProjects
      .mockResolvedValueOnce(mockProjects)
      .mockResolvedValueOnce([updatedProject, mockProjects[1]]);
    mockUpdateProject.mockResolvedValue(updatedProject);

    const { result } = renderHook(() => useProjects(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const updateRequest: UpdateProjectRequest = {
      name: 'Updated Name',
      status: 'on_hold',
    };

    await result.current.updateProject('1', updateRequest);

    await waitFor(() => {
      const project = result.current.projects.find((p) => p.id === '1');
      expect(project?.name).toBe('Updated Name');
      expect(project?.status).toBe('on_hold');
    });

    expect(mockUpdateProject).toHaveBeenCalledWith('1', updateRequest);
  });

  it('should delete project', async () => {
    // First call returns initial projects, second call (after refetch) returns list without deleted project
    mockGetProjects
      .mockResolvedValueOnce(mockProjects)
      .mockResolvedValueOnce([mockProjects[1]]);
    mockDeleteProject.mockResolvedValue(undefined);

    const { result } = renderHook(() => useProjects(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.deleteProject('1');

    await waitFor(() => {
      expect(result.current.projects).not.toContainEqual(mockProjects[0]);
    });

    expect(mockDeleteProject).toHaveBeenCalledWith('1');
  });

  it('should refetch projects', async () => {
    mockGetProjects.mockResolvedValue(mockProjects);

    const { result } = renderHook(() => useProjects(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(mockGetProjects).toHaveBeenCalledTimes(1);

    await result.current.refetch();

    expect(mockGetProjects).toHaveBeenCalledTimes(2);
  });
});
