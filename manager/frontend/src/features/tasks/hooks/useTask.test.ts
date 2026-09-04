import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import type { Task } from '../types';

const mockTasksApi = {
  getTask: mock(),
  updateTask: mock(),
  deleteTask: mock(),
  runTask: mock(),
  cancelTaskRun: mock(),
};

mock.module('../../../api/tasks', () => ({
  tasksApi: mockTasksApi,
}));

let useTask: typeof import('./useTask').useTask;

beforeAll(async () => {
  ({ useTask } = await import('./useTask'));
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
  project_id: 'proj-1',
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

describe('useTask', () => {
  beforeEach(() => {
    mock.clearAllMocks();
  });

  it('loads task on mount', async () => {
    mockTasksApi.getTask.mockResolvedValueOnce(mockTask);

    const { result } = renderHook(() => useTask('task-1'), { wrapper: createWrapper() });

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.task).toEqual(mockTask);
    expect(result.current.error).toBeNull();
    expect(mockTasksApi.getTask).toHaveBeenCalledWith('task-1');
  });

  it('handles null id', async () => {
    const { result } = renderHook(() => useTask(null), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.task).toBeNull();
    expect(mockTasksApi.getTask).not.toHaveBeenCalled();
  });

  it('handles load error', async () => {
    mockTasksApi.getTask.mockRejectedValueOnce(new Error('Not found'));

    const { result } = renderHook(() => useTask('task-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.error).toBe('Not found');
    expect(result.current.task).toBeNull();
  });

  it('updates task', async () => {
    mockTasksApi.getTask.mockResolvedValueOnce(mockTask);
    const updatedTask = { ...mockTask, title: 'Updated Task' };
    mockTasksApi.updateTask.mockResolvedValueOnce(updatedTask);

    const { result } = renderHook(() => useTask('task-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const request = { title: 'Updated Task' };
    await act(async () => {
      await result.current.updateTask(request);
    });

    expect(mockTasksApi.updateTask).toHaveBeenCalledWith('task-1', request);
    expect(result.current.task?.title).toBe('Updated Task');
  });

  it('throws error when updating without id', async () => {
    const { result } = renderHook(() => useTask(null), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await expect(result.current.updateTask({ title: 'Updated' })).rejects.toThrow(
      'No task ID provided'
    );
  });

  it('deletes task', async () => {
    mockTasksApi.getTask.mockResolvedValueOnce(mockTask);
    mockTasksApi.deleteTask.mockResolvedValueOnce();

    const { result } = renderHook(() => useTask('task-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await result.current.deleteTask();
    });

    expect(mockTasksApi.deleteTask).toHaveBeenCalledWith('task-1');
    expect(result.current.task).toBeNull();
  });

  it('throws error when deleting without id', async () => {
    const { result } = renderHook(() => useTask(null), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await expect(result.current.deleteTask()).rejects.toThrow('No task ID provided');
  });

  it('runs task', async () => {
    mockTasksApi.getTask.mockResolvedValueOnce(mockTask);
    mockTasksApi.runTask.mockResolvedValueOnce({ run_id: 'run-1' });

    const { result } = renderHook(() => useTask('task-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const response = await result.current.runTask();

    expect(mockTasksApi.runTask).toHaveBeenCalledWith('task-1');
    expect(response).toEqual({ run_id: 'run-1' });
  });

  it('throws error when running without id', async () => {
    const { result } = renderHook(() => useTask(null), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await expect(result.current.runTask()).rejects.toThrow('No task ID provided');
  });

  it('cancels task run', async () => {
    mockTasksApi.getTask.mockResolvedValueOnce(mockTask);
    mockTasksApi.cancelTaskRun.mockResolvedValueOnce();

    const { result } = renderHook(() => useTask('task-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.cancelRun();

    expect(mockTasksApi.cancelTaskRun).toHaveBeenCalledWith('task-1');
  });

  it('throws error when cancelling without id', async () => {
    const { result } = renderHook(() => useTask(null), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await expect(result.current.cancelRun()).rejects.toThrow('No task ID provided');
  });

  it('refetches task', async () => {
    mockTasksApi.getTask.mockResolvedValueOnce(mockTask);

    const { result } = renderHook(() => useTask('task-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    mockTasksApi.getTask.mockResolvedValueOnce({ ...mockTask, title: 'Refetched' });

    act(() => {
      result.current.refetch();
    });

    await waitFor(() => {
      expect(result.current.task?.title).toBe('Refetched');
    });
  });

  it('reloads when id changes', async () => {
    mockTasksApi.getTask.mockResolvedValue(mockTask);

    const { rerender } = renderHook(({ id }: { id: string | null }) => useTask(id), {
      wrapper: createWrapper(),
      initialProps: { id: 'task-1' },
    });

    await waitFor(() => {
      expect(mockTasksApi.getTask).toHaveBeenCalledWith('task-1');
    });

    mockTasksApi.getTask.mockResolvedValue({ ...mockTask, id: 'task-2' });
    act(() => {
      rerender({ id: 'task-2' });
    });

    await waitFor(() => {
      expect(mockTasksApi.getTask).toHaveBeenCalledWith('task-2');
    });
  });
});
