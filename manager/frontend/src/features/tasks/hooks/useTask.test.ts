import { act, renderHook, waitFor } from '@testing-library/react';
import { tasksApi } from '../../../api/tasks';
import type { Task } from '../types';
import { useTask } from './useTask';

jest.mock('../../../api/tasks', () => ({
  tasksApi: {
    getTask: jest.fn(),
    updateTask: jest.fn(),
    deleteTask: jest.fn(),
    runTask: jest.fn(),
    cancelTaskRun: jest.fn(),
  },
}));

const mockTasksApi = tasksApi as jest.Mocked<typeof tasksApi>;

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
    jest.clearAllMocks();
  });

  it('loads task on mount', async () => {
    mockTasksApi.getTask.mockResolvedValueOnce(mockTask);

    const { result } = renderHook(() => useTask('task-1'));

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.task).toEqual(mockTask);
    expect(result.current.error).toBeNull();
    expect(mockTasksApi.getTask).toHaveBeenCalledWith('task-1');
  });

  it('handles null id', async () => {
    const { result } = renderHook(() => useTask(null));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.task).toBeNull();
    expect(mockTasksApi.getTask).not.toHaveBeenCalled();
  });

  it('handles load error', async () => {
    mockTasksApi.getTask.mockRejectedValueOnce(new Error('Not found'));

    const { result } = renderHook(() => useTask('task-1'));

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

    const { result } = renderHook(() => useTask('task-1'));

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
    const { result } = renderHook(() => useTask(null));

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

    const { result } = renderHook(() => useTask('task-1'));

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
    const { result } = renderHook(() => useTask(null));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await expect(result.current.deleteTask()).rejects.toThrow('No task ID provided');
  });

  it('runs task', async () => {
    mockTasksApi.getTask.mockResolvedValueOnce(mockTask);
    mockTasksApi.runTask.mockResolvedValueOnce({ run_id: 'run-1' });

    const { result } = renderHook(() => useTask('task-1'));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const response = await result.current.runTask();

    expect(mockTasksApi.runTask).toHaveBeenCalledWith('task-1');
    expect(response).toEqual({ run_id: 'run-1' });
  });

  it('throws error when running without id', async () => {
    const { result } = renderHook(() => useTask(null));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await expect(result.current.runTask()).rejects.toThrow('No task ID provided');
  });

  it('cancels task run', async () => {
    mockTasksApi.getTask.mockResolvedValueOnce(mockTask);
    mockTasksApi.cancelTaskRun.mockResolvedValueOnce();

    const { result } = renderHook(() => useTask('task-1'));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.cancelRun();

    expect(mockTasksApi.cancelTaskRun).toHaveBeenCalledWith('task-1');
  });

  it('throws error when cancelling without id', async () => {
    const { result } = renderHook(() => useTask(null));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await expect(result.current.cancelRun()).rejects.toThrow('No task ID provided');
  });

  it('refetches task', async () => {
    mockTasksApi.getTask.mockResolvedValueOnce(mockTask);

    const { result } = renderHook(() => useTask('task-1'));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    mockTasksApi.getTask.mockResolvedValueOnce({ ...mockTask, title: 'Refetched' });

    result.current.refetch();

    await waitFor(() => {
      expect(result.current.task?.title).toBe('Refetched');
    });
  });

  it('reloads when id changes', async () => {
    mockTasksApi.getTask.mockResolvedValue(mockTask);

    const { rerender } = renderHook(({ id }: { id: string | null }) => useTask(id), {
      initialProps: { id: 'task-1' },
    });

    await waitFor(() => {
      expect(mockTasksApi.getTask).toHaveBeenCalledWith('task-1');
    });

    mockTasksApi.getTask.mockResolvedValue({ ...mockTask, id: 'task-2' });
    rerender({ id: 'task-2' });

    await waitFor(() => {
      expect(mockTasksApi.getTask).toHaveBeenCalledWith('task-2');
    });
  });
});
