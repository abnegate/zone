import { type QueryKey, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { tasksApi } from '../../../api/tasks';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { awaitsTitle, TITLE_POLL_MS } from '../title';
import type { CreateTaskRequest, Task, TaskStatus, UpdateTaskRequest } from '../types';

const TASKS_KEY = ['tasks'] as const;

const MOVE_KEY = ['tasks', 'move'] as const;

const LOAD_FAILED = 'Failed to load tasks';

const NO_WORKSPACE = 'No workspace selected';

interface Move {
  id: string;
  status: TaskStatus;
}

interface Moved {
  queryKey: QueryKey;
  id: string;
  from: TaskStatus;
  to: TaskStatus;
}

function withStatus(
  tasks: Task[] | undefined,
  id: string,
  from: TaskStatus | null,
  to: TaskStatus
) {
  return tasks?.map((task) =>
    task.id === id && (from === null || task.status === from) ? { ...task, status: to } : task
  );
}

export function useTasks(projectId?: string) {
  const queryClient = useQueryClient();
  const { currentWorkspace, loading: workspaceLoading } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const queryKey = ['tasks', workspaceId, projectId];

  const {
    data: tasks = [],
    isLoading,
    error,
    refetch,
  } = useQuery({
    queryKey,
    queryFn: () => {
      if (!workspaceId) {
        return [];
      }
      return tasksApi.getTasks(workspaceId, projectId);
    },
    enabled: !!workspaceId,
    refetchInterval: (query) =>
      (query.state.data as Task[] | undefined)?.some((task) => awaitsTitle(task))
        ? TITLE_POLL_MS
        : false,
  });

  const createTaskMutation = useMutation({
    mutationFn: (request: CreateTaskRequest) => {
      if (!workspaceId) throw new Error(NO_WORKSPACE);
      return tasksApi.createTask(workspaceId, request);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: TASKS_KEY });
    },
  });

  const updateTaskMutation = useMutation({
    mutationFn: ({ id, request }: { id: string; request: UpdateTaskRequest }) =>
      tasksApi.updateTask(id, request),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: TASKS_KEY }),
  });

  const moveTaskMutation = useMutation({
    mutationKey: MOVE_KEY,
    mutationFn: ({ id, status }: Move) => tasksApi.updateTask(id, { status }),
    onMutate: async ({ id, status }: Move): Promise<Moved | undefined> => {
      const moving = queryKey;
      await queryClient.cancelQueries({ queryKey: moving });
      const from = queryClient.getQueryData<Task[]>(moving)?.find((task) => task.id === id)?.status;
      queryClient.setQueryData<Task[]>(moving, (current) => withStatus(current, id, null, status));
      return from === undefined ? undefined : { queryKey: moving, id, from, to: status };
    },
    onError: (_error, _move, moved) => {
      if (!moved) return;
      queryClient.setQueryData<Task[]>(moved.queryKey, (current) =>
        withStatus(current, moved.id, moved.to, moved.from)
      );
    },
    onSettled: () => {
      if (queryClient.isMutating({ mutationKey: MOVE_KEY }) > 1) return;
      queryClient.invalidateQueries({ queryKey: TASKS_KEY });
    },
  });

  const deleteTaskMutation = useMutation({
    mutationFn: (id: string) => tasksApi.deleteTask(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: TASKS_KEY });
    },
  });

  const { mutateAsync: update } = updateTaskMutation;
  const { mutateAsync: move } = moveTaskMutation;

  const updateTask = useCallback(
    (id: string, request: UpdateTaskRequest) => update({ id, request }),
    [update]
  );

  const moveTask = useCallback((id: string, status: TaskStatus) => move({ id, status }), [move]);

  return {
    tasks,
    loading: workspaceId ? isLoading : workspaceLoading,
    error: error instanceof Error ? error.message : error ? LOAD_FAILED : null,
    createTask: createTaskMutation.mutateAsync,
    updateTask,
    moveTask,
    deleteTask: deleteTaskMutation.mutateAsync,
    refetch,
  };
}
