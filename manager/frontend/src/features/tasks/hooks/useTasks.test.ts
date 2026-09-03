import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import type { Task } from '../types';

const mockGetTasks = mock();
const mockCreateTask = mock();
const mockUpdateTask = mock();
const mockDeleteTask = mock();

mock.module('../../../api/tasks', () => ({
  tasksApi: {
    getTasks: mockGetTasks,
    createTask: mockCreateTask,
    updateTask: mockUpdateTask,
    deleteTask: mockDeleteTask,
  },
}));

// Mock the workspace context
const mockWorkspace = {
  id: 'workspace-1',
  name: 'Test Workspace',
  organization_id: 'org-1',
  slug: 'test-workspace',
  description: null,
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentWorkspace: mockWorkspace,
    loading: false,
  }),
}));

let useTasks: typeof import('./useTasks').useTasks;

beforeAll(async () => {
  ({ useTasks } = await import('./useTasks'));
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

  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
};

const mockTask: Task = {
  id: 'task-1',
  workspace_id: 'workspace-1',
  project_ids: ['proj-1'],
  title: 'Test Task',
  description: 'Test description',
  acceptance_criteria: null,
  status: 'created',
  priority: 1,
  model_name: null,
  dependencies: [],
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
  started_at: null,
  completed_at: null,
  is_agentic: false,
  github_repo_url: null,
  source_id: null,
  source_ids: [],
  queued_at: null,
  worker_id: null,
  pr_url: null,
  branch_name: null,
  pr_status: null,
  pr_created_at: null,
};

describe('useTasks', () => {
  beforeEach(() => {
    mockGetTasks.mockReset();
    mockCreateTask.mockReset();
    mockUpdateTask.mockReset();
    mockDeleteTask.mockReset();
    mockGetTasks.mockResolvedValue([]);
  });

  it('loads tasks on mount', async () => {
    mockGetTasks.mockResolvedValueOnce([mockTask]);
    const wrapper = createWrapper();

    const { result } = renderHook(() => useTasks(), { wrapper });

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.tasks).toEqual([mockTask]);
    expect(result.current.error).toBeNull();
    expect(mockGetTasks).toHaveBeenCalledWith('workspace-1', undefined, undefined);
  });

  it('loads tasks with filters', async () => {
    mockGetTasks.mockResolvedValueOnce([mockTask]);
    const wrapper = createWrapper();

    const { result } = renderHook(() => useTasks('proj-1', 'created'), { wrapper });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(mockGetTasks).toHaveBeenCalledWith('workspace-1', 'proj-1', 'created');
  });

  it('handles load error', async () => {
    mockGetTasks.mockRejectedValueOnce(new Error('Network error'));
    const wrapper = createWrapper();

    const { result } = renderHook(() => useTasks(), { wrapper });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.error).toBe('Network error');
    expect(result.current.tasks).toEqual([]);
  });

  it('creates a task', async () => {
    mockGetTasks.mockResolvedValueOnce([]);
    const newTask = { ...mockTask, id: 'task-2', title: 'New Task' };
    mockGetTasks.mockResolvedValueOnce([newTask]);
    mockCreateTask.mockResolvedValueOnce(newTask);
    const wrapper = createWrapper();

    const { result } = renderHook(() => useTasks(), { wrapper });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const request = {
      project_ids: ['proj-1'],
      title: 'New Task',
      description: 'Description',
    };

    await act(async () => {
      await result.current.createTask(request);
    });

    expect(mockCreateTask).toHaveBeenCalledWith('workspace-1', request);
    await waitFor(() => {
      expect(result.current.tasks).toEqual([newTask]);
    });
  });

  it('updates a task', async () => {
    mockGetTasks.mockResolvedValueOnce([mockTask]);
    const updatedTask = { ...mockTask, title: 'Updated Task' };
    mockGetTasks.mockResolvedValueOnce([updatedTask]);
    mockUpdateTask.mockResolvedValueOnce(updatedTask);
    const wrapper = createWrapper();

    const { result } = renderHook(() => useTasks(), { wrapper });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const request = { title: 'Updated Task' };
    await act(async () => {
      await result.current.updateTask('task-1', request);
    });

    expect(mockUpdateTask).toHaveBeenCalledWith('task-1', request);
    await waitFor(() => {
      expect(result.current.tasks[0].title).toBe('Updated Task');
    });
  });

  it('deletes a task', async () => {
    mockGetTasks.mockResolvedValueOnce([mockTask]);
    mockGetTasks.mockResolvedValueOnce([]);
    mockDeleteTask.mockResolvedValueOnce();
    const wrapper = createWrapper();

    const { result } = renderHook(() => useTasks(), { wrapper });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await result.current.deleteTask('task-1');
    });

    expect(mockDeleteTask).toHaveBeenCalledWith('task-1');
    await waitFor(() => {
      expect(result.current.tasks).toEqual([]);
    });
  });

  it('refetches tasks', async () => {
    mockGetTasks.mockResolvedValueOnce([mockTask]);
    const wrapper = createWrapper();

    const { result } = renderHook(() => useTasks(), { wrapper });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    mockGetTasks.mockResolvedValueOnce([mockTask, mockTask]);

    act(() => {
      result.current.refetch();
    });

    await waitFor(() => {
      expect(mockGetTasks).toHaveBeenCalledTimes(2);
    });
  });

  it('reloads when filters change', async () => {
    mockGetTasks.mockResolvedValue([mockTask]);
    const wrapper = createWrapper();

    const { rerender } = renderHook(
      ({ projectId, status }: { projectId?: string; status?: string }) =>
        useTasks(projectId, status),
      {
        initialProps: { projectId: 'proj-1', status: undefined as string | undefined },
        wrapper,
      }
    );

    await waitFor(() => {
      expect(mockGetTasks).toHaveBeenCalledWith('workspace-1', 'proj-1', undefined);
    });

    rerender({ projectId: 'proj-1', status: 'created' });

    await waitFor(() => {
      expect(mockGetTasks).toHaveBeenCalledWith('workspace-1', 'proj-1', 'created');
    });
  });
});
