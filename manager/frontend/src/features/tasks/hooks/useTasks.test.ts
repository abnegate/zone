import { afterAll, beforeAll, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import type { Task, TaskStatus } from '../types';

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

const createClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false, gcTime: 0 },
    },
  });

const createWrapper = (queryClient: QueryClient = createClient()) => {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
};

const deferred = <T>() => {
  let resolve: (value: T) => void = () => {};
  let reject: (reason: Error) => void = () => {};
  const promise = new Promise<T>((onResolve, onReject) => {
    resolve = onResolve;
    reject = onReject;
  });
  return { promise, resolve, reject };
};

const MOVED: TaskStatus = 'in_progress';
const QUERY_KEY = ['tasks', 'workspace-1', undefined];

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
    expect(mockGetTasks.mock.calls[0]).toEqual(['workspace-1', undefined]);
  });

  it('loads tasks for a project without a status filter', async () => {
    mockGetTasks.mockResolvedValueOnce([mockTask]);
    const wrapper = createWrapper();

    const { result } = renderHook(() => useTasks('proj-1'), { wrapper });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(mockGetTasks.mock.calls[0]).toEqual(['workspace-1', 'proj-1']);
    expect(mockGetTasks.mock.calls[0]).toHaveLength(2);
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

  it('drops a deleted task from every list at once, without waiting on a reload', async () => {
    const queryClient = createClient();
    mockGetTasks.mockResolvedValueOnce([mockTask]);
    mockDeleteTask.mockResolvedValueOnce();
    const { result } = renderHook(() => useTasks(), { wrapper: createWrapper(queryClient) });
    await waitFor(() => expect(result.current.tasks).toEqual([mockTask]));
    mockGetTasks.mockReturnValue(new Promise(() => {}));

    await act(async () => {
      await result.current.deleteTask('task-1');
    });

    expect(queryClient.getQueryData<Task[]>(QUERY_KEY)).toEqual([]);
    await waitFor(() => expect(result.current.tasks).toEqual([]));
  });

  it('keeps a deleted task out when a reload that began before the delete comes back', async () => {
    const queryClient = createClient();
    mockGetTasks.mockResolvedValueOnce([mockTask]);
    mockDeleteTask.mockResolvedValueOnce();
    const { result } = renderHook(() => useTasks(), { wrapper: createWrapper(queryClient) });
    await waitFor(() => expect(result.current.tasks).toEqual([mockTask]));

    const stale = deferred<Task[]>();
    mockGetTasks.mockReturnValueOnce(stale.promise);
    act(() => {
      void result.current.refetch();
    });
    await waitFor(() => expect(mockGetTasks).toHaveBeenCalledTimes(2));
    mockGetTasks.mockReturnValue(new Promise(() => {}));

    await act(async () => {
      await result.current.deleteTask('task-1');
    });
    await waitFor(() => expect(result.current.tasks).toEqual([]));
    await act(async () => {
      stale.resolve([mockTask]);
      await stale.promise;
    });

    expect(queryClient.getQueryData<Task[]>(QUERY_KEY)).toEqual([]);
    expect(result.current.tasks).toEqual([]);
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

  it('reloads when the project changes', async () => {
    mockGetTasks.mockResolvedValue([mockTask]);
    const wrapper = createWrapper();

    const { rerender } = renderHook(
      ({ projectId }: { projectId?: string }) => useTasks(projectId),
      {
        initialProps: { projectId: 'proj-1' as string | undefined },
        wrapper,
      }
    );

    await waitFor(() => {
      expect(mockGetTasks).toHaveBeenCalledWith('workspace-1', 'proj-1');
    });

    rerender({ projectId: 'proj-2' });

    await waitFor(() => {
      expect(mockGetTasks).toHaveBeenCalledWith('workspace-1', 'proj-2');
    });
  });
});

describe('useTasks moveTask', () => {
  beforeEach(() => {
    mockGetTasks.mockReset();
    mockUpdateTask.mockReset();
    mockGetTasks.mockResolvedValue([mockTask]);
  });

  const moved: Task = { ...mockTask, status: MOVED };

  const renderLoaded = async (queryClient: QueryClient = createClient()) => {
    const rendered = renderHook(() => useTasks(), { wrapper: createWrapper(queryClient) });
    await waitFor(() => {
      expect(rendered.result.current.tasks).toEqual([mockTask]);
    });
    return rendered;
  };

  it('shows the new status in the render the move is made in, leaving the cache alone', async () => {
    const queryClient = createClient();
    const update = deferred<Task>();
    mockUpdateTask.mockReturnValueOnce(update.promise);
    const { result } = await renderLoaded(queryClient);

    let move: Promise<Task> = Promise.resolve(mockTask);
    act(() => {
      move = result.current.moveTask('task-1', MOVED);
    });

    expect(result.current.tasks[0].status).toBe(MOVED);
    expect(queryClient.getQueryData<Task[]>(QUERY_KEY)).toEqual([mockTask]);
    await waitFor(() => expect(mockUpdateTask).toHaveBeenCalledTimes(1));

    mockGetTasks.mockResolvedValue([moved]);
    await act(async () => {
      update.resolve(moved);
      await expect(move).resolves.toEqual(moved);
    });
    expect(result.current.tasks[0].status).toBe(MOVED);
    expect(queryClient.getQueryData<Task[]>(QUERY_KEY)).toEqual([moved]);
  });

  it('sends only the status in the update body', async () => {
    mockUpdateTask.mockResolvedValueOnce(moved);
    const { result } = await renderLoaded();

    await act(async () => {
      await result.current.moveTask('task-1', MOVED);
    });

    expect(mockUpdateTask.mock.calls).toEqual([['task-1', { status: MOVED }]]);
    expect(Object.keys(mockUpdateTask.mock.calls[0][1])).toEqual(['status']);
  });

  it('keeps the new status when a refetch started after the move comes back stale', async () => {
    const queryClient = createClient();
    const update = deferred<Task>();
    mockUpdateTask.mockReturnValueOnce(update.promise);
    const { result } = await renderLoaded(queryClient);

    act(() => {
      void result.current.moveTask('task-1', MOVED);
    });
    const stale = deferred<Task[]>();
    mockGetTasks.mockReturnValueOnce(stale.promise);
    act(() => {
      void result.current.refetch();
    });
    await waitFor(() => expect(mockGetTasks).toHaveBeenCalledTimes(2));
    const refreshed = { ...mockTask, title: 'Retitled elsewhere' };
    await act(async () => {
      stale.resolve([refreshed]);
      await stale.promise;
    });

    await waitFor(() => expect(result.current.tasks[0].title).toBe('Retitled elsewhere'));
    expect(queryClient.getQueryData<Task[]>(QUERY_KEY)).toEqual([refreshed]);
    expect(result.current.tasks[0].status).toBe(MOVED);
  });

  it('ignores a stale refetch that is still in flight when the move succeeds', async () => {
    const queryClient = createClient();
    const update = deferred<Task>();
    mockUpdateTask.mockReturnValueOnce(update.promise);
    const { result } = await renderLoaded(queryClient);

    let move: Promise<Task> = Promise.resolve(mockTask);
    act(() => {
      move = result.current.moveTask('task-1', MOVED);
    });
    const stale = deferred<Task[]>();
    mockGetTasks.mockReturnValueOnce(stale.promise);
    act(() => {
      void result.current.refetch();
    });
    await waitFor(() => expect(mockGetTasks).toHaveBeenCalledTimes(2));

    const fresh = deferred<Task[]>();
    mockGetTasks.mockReturnValue(fresh.promise);
    await act(async () => {
      update.resolve(moved);
      await move;
    });
    await act(async () => {
      stale.resolve([mockTask]);
      await stale.promise;
    });

    expect(queryClient.getQueryData<Task[]>(QUERY_KEY)).toEqual([moved]);
    expect(result.current.tasks[0].status).toBe(MOVED);
  });

  it('returns the card and rethrows when the update is refused, without waiting on a reload', async () => {
    const queryClient = createClient();
    const { result } = await renderLoaded(queryClient);

    const reload = deferred<Task[]>();
    mockGetTasks.mockReturnValue(reload.promise);
    const update = deferred<Task>();
    mockUpdateTask.mockReturnValueOnce(update.promise);

    let move: Promise<Task> = Promise.resolve(mockTask);
    act(() => {
      move = result.current.moveTask('task-1', MOVED);
    });
    expect(result.current.tasks[0].status).toBe(MOVED);

    const refusal = new Error('Task has an active run');
    await act(async () => {
      update.reject(refusal);
      await expect(move).rejects.toBe(refusal);
    });

    expect(result.current.tasks).toEqual([mockTask]);
    expect(queryClient.getQueryData<Task[]>(QUERY_KEY)).toEqual([mockTask]);
  });

  it('invalidates the task lists once the move settles', async () => {
    const queryClient = createClient();
    const invalidate = spyOn(queryClient, 'invalidateQueries');
    const { result } = await renderLoaded(queryClient);

    mockUpdateTask.mockResolvedValueOnce(moved);
    await act(async () => {
      await result.current.moveTask('task-1', MOVED);
    });
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['tasks'] });

    mockUpdateTask.mockRejectedValueOnce(new Error('refused'));
    await act(async () => {
      await expect(result.current.moveTask('task-1', 'blocked')).rejects.toThrow('refused');
    });
    expect(invalidate).toHaveBeenCalledTimes(2);
    expect(invalidate).toHaveBeenLastCalledWith({ queryKey: ['tasks'] });
  });

  it('finishes a move whose board has gone, so the next one shows it', async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false, gcTime: Number.POSITIVE_INFINITY },
        mutations: { retry: false, gcTime: 0 },
      },
    });
    const update = deferred<Task>();
    mockUpdateTask.mockReturnValueOnce(update.promise);
    const { result, unmount } = await renderLoaded(queryClient);

    let move: Promise<Task> = Promise.resolve(mockTask);
    act(() => {
      move = result.current.moveTask('task-1', MOVED);
    });
    unmount();
    mockGetTasks.mockResolvedValue([moved]);
    await act(async () => {
      update.resolve(moved);
      await move;
    });

    expect(queryClient.getQueryData<Task[]>(QUERY_KEY)).toEqual([moved]);
    const again = renderHook(() => useTasks(), { wrapper: createWrapper(queryClient) });
    expect(again.result.current.tasks).toEqual([moved]);
  });

  it('keeps its callbacks across renders', async () => {
    const { result, rerender } = await renderLoaded();
    const { moveTask, updateTask } = result.current;

    rerender();

    expect(result.current.moveTask).toBe(moveTask);
    expect(result.current.updateTask).toBe(updateTask);
  });
});

describe('useTasks quick moves', () => {
  const other: Task = { ...mockTask, id: 'task-2', title: 'Other Task' };
  const BLOCKED: TaskStatus = 'blocked';
  const PROJECT = 'proj-1';
  const PROJECT_KEY = ['tasks', 'workspace-1', PROJECT];

  const keepingClient = () =>
    new QueryClient({
      defaultOptions: {
        queries: { retry: false, gcTime: Number.POSITIVE_INFINITY },
        mutations: { retry: false, gcTime: 0 },
      },
    });

  function statusOf(queryClient: QueryClient, id: string, key: unknown[] = QUERY_KEY) {
    return queryClient.getQueryData<Task[]>(key)?.find((task) => task.id === id)?.status;
  }

  async function renderBoth(queryClient: QueryClient) {
    const rendered = renderHook(({ projectId }: { projectId?: string }) => useTasks(projectId), {
      initialProps: { projectId: undefined as string | undefined },
      wrapper: createWrapper(queryClient),
    });
    await waitFor(() => {
      expect(rendered.result.current.tasks).toEqual([mockTask, other]);
    });
    return rendered;
  }

  function holdReloads() {
    const reload = deferred<Task[]>();
    mockGetTasks.mockReset();
    mockGetTasks.mockReturnValue(reload.promise);
    return reload;
  }

  function shown(result: { current: ReturnType<typeof useTasks> }) {
    return result.current.tasks.map((task) => task.status);
  }

  beforeEach(() => {
    mockGetTasks.mockReset();
    mockUpdateTask.mockReset();
    mockGetTasks.mockImplementation((_workspace: string, projectId?: string) =>
      Promise.resolve(projectId === PROJECT ? [other] : [mockTask, other])
    );
  });

  it('keeps a later move that succeeds when an earlier one is refused', async () => {
    const queryClient = keepingClient();
    const { result } = await renderBoth(queryClient);
    holdReloads();
    const first = deferred<Task>();
    const second = deferred<Task>();
    mockUpdateTask.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

    let firstMove: Promise<Task> = Promise.resolve(mockTask);
    let secondMove: Promise<Task> = Promise.resolve(other);
    act(() => {
      firstMove = result.current.moveTask('task-1', MOVED);
    });
    act(() => {
      secondMove = result.current.moveTask('task-2', BLOCKED);
    });
    expect(shown(result)).toEqual([MOVED, BLOCKED]);

    await act(async () => {
      first.reject(new Error('refused'));
      await expect(firstMove).rejects.toThrow('refused');
    });
    expect(shown(result)).toEqual(['created', BLOCKED]);

    await act(async () => {
      second.resolve({ ...other, status: BLOCKED });
      await secondMove;
    });
    expect(shown(result)).toEqual(['created', BLOCKED]);
    expect(statusOf(queryClient, 'task-1')).toBe('created');
    expect(statusOf(queryClient, 'task-2')).toBe(BLOCKED);
  });

  it('shows the latest of two moves of one card whichever settles first', async () => {
    const queryClient = keepingClient();
    const { result } = await renderBoth(queryClient);
    holdReloads();
    const first = deferred<Task>();
    const second = deferred<Task>();
    mockUpdateTask.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

    let firstMove: Promise<Task> = Promise.resolve(mockTask);
    let secondMove: Promise<Task> = Promise.resolve(mockTask);
    act(() => {
      firstMove = result.current.moveTask('task-1', MOVED);
    });
    act(() => {
      secondMove = result.current.moveTask('task-1', BLOCKED);
    });
    expect(shown(result)).toEqual([BLOCKED, 'created']);

    await act(async () => {
      first.resolve({ ...mockTask, status: MOVED });
      await firstMove;
    });
    expect(shown(result)).toEqual([BLOCKED, 'created']);
    expect(statusOf(queryClient, 'task-1')).toBe(MOVED);

    await act(async () => {
      second.reject(new Error('refused'));
      await expect(secondMove).rejects.toThrow('refused');
    });
    expect(shown(result)).toEqual([MOVED, 'created']);
  });

  it('carries a move across a change of filter and returns the card when it is refused', async () => {
    const queryClient = keepingClient();
    const { result, rerender } = await renderBoth(queryClient);
    const update = deferred<Task>();
    mockUpdateTask.mockReturnValueOnce(update.promise);

    let move: Promise<Task> = Promise.resolve(other);
    act(() => {
      move = result.current.moveTask('task-2', BLOCKED);
    });
    rerender({ projectId: PROJECT });
    await waitFor(() => expect(result.current.tasks).toEqual([{ ...other, status: BLOCKED }]));
    holdReloads();

    await act(async () => {
      update.reject(new Error('refused'));
      await expect(move).rejects.toThrow('refused');
    });

    expect(result.current.tasks).toEqual([other]);
    expect(queryClient.getQueryData<Task[]>(PROJECT_KEY)).toEqual([other]);
    expect(statusOf(queryClient, 'task-2')).toBe('created');
  });

  it('records an accepted move in every list that holds the card', async () => {
    const queryClient = keepingClient();
    const { result, rerender } = await renderBoth(queryClient);
    const update = deferred<Task>();
    mockUpdateTask.mockReturnValueOnce(update.promise);

    let move: Promise<Task> = Promise.resolve(other);
    act(() => {
      move = result.current.moveTask('task-2', BLOCKED);
    });
    rerender({ projectId: PROJECT });
    await waitFor(() => expect(result.current.tasks).toHaveLength(1));
    holdReloads();

    await act(async () => {
      update.resolve({ ...other, status: BLOCKED });
      await move;
    });

    expect(statusOf(queryClient, 'task-2', PROJECT_KEY)).toBe(BLOCKED);
    expect(statusOf(queryClient, 'task-2')).toBe(BLOCKED);
    expect(shown(result)).toEqual([BLOCKED]);
  });
});
