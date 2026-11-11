import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import { TITLE_POLL_MS } from '../title';
import type { Task } from '../types';

const mockGetTasks = mock();

mock.module('../../../api/tasks', () => ({
  tasksApi: { getTasks: mockGetTasks },
}));

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: { id: 'workspace-1' }, loading: false }),
}));

let useWorkspaceTasks: typeof import('./useWorkspaceTasks').useWorkspaceTasks;

beforeAll(async () => {
  ({ useWorkspaceTasks } = await import('./useWorkspaceTasks'));
});

afterAll(() => {
  mock.restore();
});

const task: Task = {
  id: 'task-1',
  workspace_id: 'workspace-1',
  project_ids: ['project-2'],
  title: 'Users on Safari are sent back to the login page',
  title_pending: true,
  description: 'Users on Safari are sent back to the login page every time they sign in',
  acceptance_criteria: null,
  status: 'created',
  priority: null,
  model_name: null,
  dependencies: [],
  created_at: new Date().toISOString(),
  updated_at: null,
  started_at: null,
  completed_at: null,
  is_agentic: true,
  require_plan_approval: false,
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

function provider(client: QueryClient = new QueryClient(QUIET)) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
}

const QUIET = { defaultOptions: { queries: { retry: false, gcTime: 0 } } };

describe('useWorkspaceTasks', () => {
  beforeEach(() => {
    mockGetTasks.mockReset();
  });

  it('reads nothing while the board is not filtered', async () => {
    const { result } = renderHook(() => useWorkspaceTasks(false), { wrapper: provider() });
    await new Promise((settle) => setTimeout(settle, 50));
    expect(mockGetTasks).not.toHaveBeenCalled();
    expect(result.current.tasks).toBeUndefined();
  });

  it('reads every task of the workspace', async () => {
    mockGetTasks.mockResolvedValue([task]);
    const { result } = renderHook(() => useWorkspaceTasks(true), { wrapper: provider() });
    await waitFor(() => expect(result.current.tasks).toEqual([task]));
    expect(mockGetTasks).toHaveBeenCalledWith('workspace-1');
  });

  it(
    'reads again while a task waits for its generated title',
    async () => {
      mockGetTasks.mockResolvedValue([task]);
      renderHook(() => useWorkspaceTasks(true), { wrapper: provider() });
      await waitFor(() => expect(mockGetTasks).toHaveBeenCalledTimes(2), {
        timeout: TITLE_POLL_MS * 2,
      });
    },
    TITLE_POLL_MS * 4
  );

  it('reports a failed read', async () => {
    mockGetTasks.mockRejectedValue(new Error('The workspace could not be read'));
    const { result } = renderHook(() => useWorkspaceTasks(true), { wrapper: provider() });
    await waitFor(() => expect(result.current.error).toBe('The workspace could not be read'));
    expect(result.current.tasks).toBeUndefined();
  });

  it('keeps the tasks it read when a later read fails, without reporting it', async () => {
    const client = new QueryClient(QUIET);
    const settled = { ...task, title_pending: false };
    mockGetTasks.mockResolvedValue([settled]);
    let renders = 0;
    const { result } = renderHook(
      () => {
        renders += 1;
        return useWorkspaceTasks(true);
      },
      { wrapper: provider(client) }
    );
    await waitFor(() => expect(result.current.tasks).toEqual([settled]));
    const before = renders;

    mockGetTasks.mockRejectedValue(new Error('The workspace could not be read'));
    await act(() => client.refetchQueries());
    await waitFor(() => expect(renders).toBeGreaterThan(before));

    expect(mockGetTasks).toHaveBeenCalledTimes(2);
    expect(result.current.tasks).toEqual([settled]);
    expect(result.current.error).toBeNull();
  });
});
