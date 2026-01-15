import { act, renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { tasksApi } from '../../../api/tasks';
import type { Task } from '../types';
import { useTasks } from './useTasks';
import { createElement } from 'react';
import type { ReactNode } from 'react';
import { WorkspaceProvider } from '../../../shared/context/WorkspaceContext';

jest.mock('../../../api/tasks', () => ({
  tasksApi: {
    getTasks: jest.fn(),
    createTask: jest.fn(),
    updateTask: jest.fn(),
    deleteTask: jest.fn(),
  },
}));

const mockTasksApi = tasksApi as jest.Mocked<typeof tasksApi>;

const mockWorkspace = {
  id: 'workspace-1',
  name: 'Test Workspace',
  organization_id: 'org-1',
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  return ({ children }: { children: ReactNode }) =>
    createElement(
      QueryClientProvider,
      { client: queryClient },
      createElement(WorkspaceProvider, { initialWorkspace: mockWorkspace }, children)
    );
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
    jest.clearAllMocks();
    mockTasksApi.getTasks.mockResolvedValue([]);
  });

  it('loads tasks on mount', async () => {
    mockTasksApi.getTasks.mockResolvedValueOnce([mockTask]);
    const wrapper = createWrapper();

    const { result } = renderHook(() => useTasks(), { wrapper });

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.tasks).toEqual([mockTask]);
    expect(result.current.error).toBeNull();
    expect(mockTasksApi.getTasks).toHaveBeenCalledWith('workspace-1', undefined, undefined);
  });

  it('loads tasks with filters', async () => {
    mockTasksApi.getTasks.mockResolvedValueOnce([mockTask]);
    const wrapper = createWrapper();

    const { result } = renderHook(() => useTasks('proj-1', 'created'), { wrapper });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(mockTasksApi.getTasks).toHaveBeenCalledWith('workspace-1', 'proj-1', 'created');
  });

  it('handles load error', async () => {
    mockTasksApi.getTasks.mockRejectedValueOnce(new Error('Network error'));
    const wrapper = createWrapper();

    const { result } = renderHook(() => useTasks(), { wrapper });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.error).toBe('Network error');
    expect(result.current.tasks).toEqual([]);
  });

  it('creates a task', async () => {
    mockTasksApi.getTasks.mockResolvedValueOnce([]);
    const newTask = { ...mockTask, id: 'task-2', title: 'New Task' };
    mockTasksApi.getTasks.mockResolvedValueOnce([newTask]);
    mockTasksApi.createTask.mockResolvedValueOnce(newTask);
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

    expect(mockTasksApi.createTask).toHaveBeenCalledWith('workspace-1', request);
    await waitFor(() => {
      expect(result.current.tasks).toEqual([newTask]);
    });
  });

  it('updates a task', async () => {
    mockTasksApi.getTasks.mockResolvedValueOnce([mockTask]);
    const updatedTask = { ...mockTask, title: 'Updated Task' };
    mockTasksApi.getTasks.mockResolvedValueOnce([updatedTask]);
    mockTasksApi.updateTask.mockResolvedValueOnce(updatedTask);
    const wrapper = createWrapper();

    const { result } = renderHook(() => useTasks(), { wrapper });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const request = { title: 'Updated Task' };
    await act(async () => {
      await result.current.updateTask('task-1', request);
    });

    expect(mockTasksApi.updateTask).toHaveBeenCalledWith('task-1', request);
    await waitFor(() => {
      expect(result.current.tasks[0].title).toBe('Updated Task');
    });
  });

  it('deletes a task', async () => {
    mockTasksApi.getTasks.mockResolvedValueOnce([mockTask]);
    mockTasksApi.getTasks.mockResolvedValueOnce([]);
    mockTasksApi.deleteTask.mockResolvedValueOnce();
    const wrapper = createWrapper();

    const { result } = renderHook(() => useTasks(), { wrapper });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await result.current.deleteTask('task-1');
    });

    expect(mockTasksApi.deleteTask).toHaveBeenCalledWith('task-1');
    await waitFor(() => {
      expect(result.current.tasks).toEqual([]);
    });
  });

  it('refetches tasks', async () => {
    mockTasksApi.getTasks.mockResolvedValueOnce([mockTask]);
    const wrapper = createWrapper();

    const { result } = renderHook(() => useTasks(), { wrapper });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    mockTasksApi.getTasks.mockResolvedValueOnce([mockTask, mockTask]);

    act(() => {
      result.current.refetch();
    });

    await waitFor(() => {
      expect(mockTasksApi.getTasks).toHaveBeenCalledTimes(2);
    });
  });

  it('reloads when filters change', async () => {
    mockTasksApi.getTasks.mockResolvedValue([mockTask]);
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
      expect(mockTasksApi.getTasks).toHaveBeenCalledWith('workspace-1', 'proj-1', undefined);
    });

    rerender({ projectId: 'proj-1', status: 'created' });

    await waitFor(() => {
      expect(mockTasksApi.getTasks).toHaveBeenCalledWith('workspace-1', 'proj-1', 'created');
    });
  });
});
