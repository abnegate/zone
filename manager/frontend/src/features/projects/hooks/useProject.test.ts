import { renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import type { Project } from '../types';
import { createElement } from 'react';
import type { ReactNode } from 'react';

const mockGetProject = mock();

mock.module('../../../api/projects', () => ({
  projectsApi: {
    getProject: mockGetProject,
  },
}));

let useProject: typeof import('./useProject').useProject;

beforeAll(async () => {
  ({ useProject } = await import('./useProject'));
});

afterAll(() => {
  mock.restore();
});

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
    mockGetProject.mockReset();
  });

  it('should fetch project on mount', async () => {
    mockGetProject.mockResolvedValue(mockProject);

    const { result } = renderHook(() => useProject('1'), { wrapper: createWrapper() });

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.project).toEqual(mockProject);
    expect(result.current.error).toBeNull();
    expect(mockGetProject).toHaveBeenCalledWith('1');
  });

  it('should handle fetch error', async () => {
    const error = new Error('Project not found');
    mockGetProject.mockRejectedValue(error);

    const { result } = renderHook(() => useProject('999'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.project).toBeNull();
    expect(result.current.error).toBe('Project not found');
  });

  it('should refetch project', async () => {
    mockGetProject.mockResolvedValue(mockProject);

    const { result } = renderHook(() => useProject('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(mockGetProject).toHaveBeenCalledTimes(1);

    await result.current.refetch();

    expect(mockGetProject).toHaveBeenCalledTimes(2);
  });

  it('should update local state when project is updated externally', async () => {
    mockGetProject.mockResolvedValue(mockProject);

    const { result } = renderHook(() => useProject('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const updatedProject: Project = {
      ...mockProject,
      name: 'Updated Name',
    };

    mockGetProject.mockResolvedValue(updatedProject);

    await result.current.refetch();

    await waitFor(() => {
      expect(result.current.project?.name).toBe('Updated Name');
    });
  });

  it('should not fetch if id is null', () => {
    const { result } = renderHook(() => useProject(null), { wrapper: createWrapper() });

    expect(result.current.project).toBeNull();
    expect(result.current.loading).toBe(false);
    expect(mockGetProject).not.toHaveBeenCalled();
  });
});
