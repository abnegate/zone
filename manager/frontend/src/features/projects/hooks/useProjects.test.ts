import { renderHook, waitFor } from '@testing-library/react';
import { useProjects } from './useProjects';
import { projectsApi } from '../../../api/projects';
import type { Project, CreateProjectRequest, UpdateProjectRequest } from '../types';

jest.mock('../../../api/projects');

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
    jest.clearAllMocks();
  });

  it('should fetch projects on mount', async () => {
    (projectsApi.getProjects as jest.Mock as jest.Mock).mockResolvedValue(mockProjects);

    const { result } = renderHook(() => useProjects());

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.projects).toEqual(mockProjects);
    expect(result.current.error).toBeNull();
    expect(projectsApi.getProjects).toHaveBeenCalledWith(undefined);
  });

  it('should fetch projects with status filter', async () => {
    (projectsApi.getProjects as jest.Mock).mockResolvedValue([mockProjects[0]]);

    const { result } = renderHook(() => useProjects('active'));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.projects).toEqual([mockProjects[0]]);
    expect(projectsApi.getProjects).toHaveBeenCalledWith('active');
  });

  it('should handle fetch error', async () => {
    const error = new Error('Failed to fetch');
    (projectsApi.getProjects as jest.Mock).mockRejectedValue(error);

    const { result } = renderHook(() => useProjects());

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

    (projectsApi.getProjects as jest.Mock).mockResolvedValue(mockProjects);
    (projectsApi.createProject as jest.Mock).mockResolvedValue(newProject);

    const { result } = renderHook(() => useProjects());

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

    expect(projectsApi.createProject).toHaveBeenCalledWith(createRequest);
  });

  it('should update project', async () => {
    const updatedProject: Project = {
      ...mockProjects[0],
      name: 'Updated Name',
      status: 'on_hold',
    };

    (projectsApi.getProjects as jest.Mock).mockResolvedValue(mockProjects);
    (projectsApi.updateProject as jest.Mock).mockResolvedValue(updatedProject);

    const { result } = renderHook(() => useProjects());

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

    expect(projectsApi.updateProject).toHaveBeenCalledWith('1', updateRequest);
  });

  it('should delete project', async () => {
    (projectsApi.getProjects as jest.Mock).mockResolvedValue(mockProjects);
    (projectsApi.deleteProject as jest.Mock).mockResolvedValue(undefined);

    const { result } = renderHook(() => useProjects());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.deleteProject('1');

    await waitFor(() => {
      expect(result.current.projects).not.toContainEqual(mockProjects[0]);
    });

    expect(projectsApi.deleteProject).toHaveBeenCalledWith('1');
  });

  it('should refetch projects', async () => {
    (projectsApi.getProjects as jest.Mock).mockResolvedValue(mockProjects);

    const { result } = renderHook(() => useProjects());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(projectsApi.getProjects).toHaveBeenCalledTimes(1);

    await result.current.refetch();

    expect(projectsApi.getProjects).toHaveBeenCalledTimes(2);
  });
});
