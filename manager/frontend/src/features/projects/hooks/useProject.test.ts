import { renderHook, waitFor } from '@testing-library/react';
import { useProject } from './useProject';
import { projectsApi } from '../../../api/projects';
import type { Project } from '../types';

jest.mock('../../../api/projects');

const mockProject: Project = {
  id: '1',
  name: 'Test Project',
  description: 'Description',
  status: 'active',
  github_repo_url: null,
  source_id: 'src-1',
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-02T00:00:00Z',
};

describe('useProject', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('should fetch project on mount', async () => {
    (projectsApi.getProject as jest.Mock).mockResolvedValue(mockProject);

    const { result } = renderHook(() => useProject('1'));

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.project).toEqual(mockProject);
    expect(result.current.error).toBeNull();
    expect(projectsApi.getProject).toHaveBeenCalledWith('1');
  });

  it('should handle fetch error', async () => {
    const error = new Error('Project not found');
    (projectsApi.getProject as jest.Mock).mockRejectedValue(error);

    const { result } = renderHook(() => useProject('999'));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.project).toBeNull();
    expect(result.current.error).toBe('Project not found');
  });

  it('should refetch project', async () => {
    (projectsApi.getProject as jest.Mock).mockResolvedValue(mockProject);

    const { result } = renderHook(() => useProject('1'));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(projectsApi.getProject).toHaveBeenCalledTimes(1);

    await result.current.refetch();

    expect(projectsApi.getProject).toHaveBeenCalledTimes(2);
  });

  it('should update local state when project is updated externally', async () => {
    (projectsApi.getProject as jest.Mock).mockResolvedValue(mockProject);

    const { result } = renderHook(() => useProject('1'));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const updatedProject: Project = {
      ...mockProject,
      name: 'Updated Name',
    };

    (projectsApi.getProject as jest.Mock).mockResolvedValue(updatedProject);

    await result.current.refetch();

    await waitFor(() => {
      expect(result.current.project?.name).toBe('Updated Name');
    });
  });

  it('should not fetch if id is null', () => {
    const { result } = renderHook(() => useProject(null));

    expect(result.current.project).toBeNull();
    expect(result.current.loading).toBe(false);
    expect(projectsApi.getProject).not.toHaveBeenCalled();
  });
});
