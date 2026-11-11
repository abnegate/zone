import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback, useMemo, useState } from 'react';
import { tasksApi } from '../../../api/tasks';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { titlePoll } from '../title';
import type { CreateTaskRequest, Task, TaskStatus, UpdateTaskRequest } from '../types';

export const TASKS_KEY = ['tasks'] as const;

export function tasksKey(workspaceId?: string, projectId?: string) {
  return [...TASKS_KEY, workspaceId, projectId] as const;
}

export const LOAD_FAILED = 'Failed to load tasks';

const NO_WORKSPACE = 'No workspace selected';

const NO_TASKS: Task[] = [];

const NONE: Moves = new Map();

interface Move {
  id: string;
  status: TaskStatus;
}

type Moves = ReadonlyMap<string, Move>;

function without(moves: Moves, move: Move): Moves {
  if (moves.get(move.id) !== move) return moves;
  const rest = new Map(moves);
  rest.delete(move.id);
  return rest;
}

export function useTasks(projectId?: string) {
  const queryClient = useQueryClient();
  const { currentWorkspace, loading: workspaceLoading } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const queryKey = tasksKey(workspaceId, projectId);

  const [moves, setMoves] = useState(NONE);

  const {
    data: listed = NO_TASKS,
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
    refetchInterval: (query) => titlePoll(query.state.data),
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
    mutationFn: ({ id, status }: Move) => tasksApi.updateTask(id, { status }),
    onSuccess: async (moved) => {
      await queryClient.cancelQueries({ queryKey: TASKS_KEY });
      queryClient.setQueriesData<Task[]>({ queryKey: TASKS_KEY }, (current) =>
        current?.map((task) => (task.id === moved.id ? moved : task))
      );
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: TASKS_KEY });
    },
  });

  const deleteTaskMutation = useMutation({
    mutationFn: (id: string) => tasksApi.deleteTask(id),
    onSuccess: async (_deleted, id) => {
      await queryClient.cancelQueries({ queryKey: TASKS_KEY });
      queryClient.setQueriesData<Task[]>({ queryKey: TASKS_KEY }, (current) =>
        current?.filter((task) => task.id !== id)
      );
      void queryClient.invalidateQueries({ queryKey: TASKS_KEY });
    },
  });

  const { mutateAsync: update } = updateTaskMutation;
  const { mutateAsync: move } = moveTaskMutation;

  const updateTask = useCallback(
    (id: string, request: UpdateTaskRequest) => update({ id, request }),
    [update]
  );

  const moveTask = useCallback(
    async (id: string, status: TaskStatus) => {
      const pending: Move = { id, status };
      setMoves((current) => new Map(current).set(id, pending));
      try {
        return await move(pending);
      } finally {
        setMoves((current) => without(current, pending));
      }
    },
    [move]
  );

  const tasks = useMemo(() => {
    if (moves.size === 0) return listed;
    return listed.map((task) => {
      const pending = moves.get(task.id);
      return pending && pending.status !== task.status ? { ...task, status: pending.status } : task;
    });
  }, [listed, moves]);

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
