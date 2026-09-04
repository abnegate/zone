import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import type { TaskRun, TaskRunLog } from '../types';

const mockTasksApi = {
  getTaskRuns: mock(),
  getTaskRun: mock(),
  getTaskRunLogs: mock(),
};

mock.module('../../../api/tasks', () => ({
  tasksApi: mockTasksApi,
}));

let useTaskRuns: typeof import('./useTaskRuns').useTaskRuns;

beforeAll(async () => {
  ({ useTaskRuns } = await import('./useTaskRuns'));
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

const mockRun: TaskRun = {
  id: 'run-1',
  task_id: 'task-1',
  status: 'running',
  current_phase: 'planning',
  progress_percent: 25,
  error_message: null,
  started_at: '2024-01-01T00:00:00Z',
  completed_at: null,
};

const mockLog: TaskRunLog = {
  id: 'log-1',
  run_id: 'run-1',
  phase: 'planning',
  agent_type: 'architect',
  level: 'info',
  message: 'Starting planning phase',
  created_at: '2024-01-01T00:00:00Z',
};

describe('useTaskRuns', () => {
  beforeEach(() => {
    mock.clearAllMocks();
  });

  it('loads runs on mount', async () => {
    mockTasksApi.getTaskRuns.mockResolvedValueOnce([mockRun]);

    const { result } = renderHook(() => useTaskRuns('task-1'), { wrapper: createWrapper() });

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.runs).toEqual([mockRun]);
    expect(result.current.error).toBeNull();
    expect(mockTasksApi.getTaskRuns).toHaveBeenCalledWith('task-1');
  });

  it('handles null taskId', async () => {
    const { result } = renderHook(() => useTaskRuns(null), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.runs).toEqual([]);
    expect(mockTasksApi.getTaskRuns).not.toHaveBeenCalled();
  });

  it('handles load error', async () => {
    mockTasksApi.getTaskRuns.mockRejectedValueOnce(new Error('Failed to fetch'));

    const { result } = renderHook(() => useTaskRuns('task-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.error).toBe('Failed to fetch');
    expect(result.current.runs).toEqual([]);
  });

  it('gets a single run', async () => {
    mockTasksApi.getTaskRuns.mockResolvedValueOnce([mockRun]);
    mockTasksApi.getTaskRun.mockResolvedValueOnce(mockRun);

    const { result } = renderHook(() => useTaskRuns('task-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const run = await result.current.getRun('run-1');

    expect(mockTasksApi.getTaskRun).toHaveBeenCalledWith('task-1', 'run-1');
    expect(run).toEqual(mockRun);
  });

  it('throws error when getting run without taskId', async () => {
    const { result } = renderHook(() => useTaskRuns(null), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await expect(result.current.getRun('run-1')).rejects.toThrow('No task ID provided');
  });

  it('gets run logs', async () => {
    mockTasksApi.getTaskRuns.mockResolvedValueOnce([mockRun]);
    mockTasksApi.getTaskRunLogs.mockResolvedValueOnce([mockLog]);

    const { result } = renderHook(() => useTaskRuns('task-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const logs = await result.current.getRunLogs('run-1');

    expect(mockTasksApi.getTaskRunLogs).toHaveBeenCalledWith('task-1', 'run-1');
    expect(logs).toEqual([mockLog]);
  });

  it('throws error when getting logs without taskId', async () => {
    const { result } = renderHook(() => useTaskRuns(null), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await expect(result.current.getRunLogs('run-1')).rejects.toThrow('No task ID provided');
  });

  it('refetches runs', async () => {
    mockTasksApi.getTaskRuns.mockResolvedValueOnce([mockRun]);

    const { result } = renderHook(() => useTaskRuns('task-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const newRun = { ...mockRun, id: 'run-2' };
    mockTasksApi.getTaskRuns.mockResolvedValueOnce([mockRun, newRun]);

    act(() => {
      result.current.refetch();
    });

    await waitFor(() => {
      expect(result.current.runs).toHaveLength(2);
    });
  });

  it('reloads when taskId changes', async () => {
    mockTasksApi.getTaskRuns.mockResolvedValue([mockRun]);

    const { rerender } = renderHook(
      ({ taskId }: { taskId: string | null }) => useTaskRuns(taskId),
      {
        wrapper: createWrapper(),
        initialProps: { taskId: 'task-1' },
      }
    );

    await waitFor(() => {
      expect(mockTasksApi.getTaskRuns).toHaveBeenCalledWith('task-1');
    });

    act(() => {
      rerender({ taskId: 'task-2' });
    });

    await waitFor(() => {
      expect(mockTasksApi.getTaskRuns).toHaveBeenCalledWith('task-2');
    });
  });
});
