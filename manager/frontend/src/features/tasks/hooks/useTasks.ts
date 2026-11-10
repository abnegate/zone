import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { tasksApi } from '../../../api/tasks';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { awaitsTitle, TITLE_POLL_MS } from '../title';
import type { CreateTaskRequest, Task, TaskStatus, UpdateTaskRequest } from '../types';

interface Move {
  id: string;
  status: TaskStatus;
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
      if (!workspaceId) throw new Error('No workspace selected');
      return tasksApi.createTask(workspaceId, request);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['tasks'] });
    },
  });

  const updateTaskMutation = useMutation({
    mutationFn: ({ id, request }: { id: string; request: UpdateTaskRequest }) =>
      tasksApi.updateTask(id, request),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['tasks'] }),
  });

  const moveTaskMutation = useMutation({
    mutationFn: ({ id, status }: Move) => tasksApi.updateTask(id, { status }),
    onMutate: async ({ id, status }: Move) => {
      await queryClient.cancelQueries({ queryKey });
      const previous = queryClient.getQueryData<Task[]>(queryKey);
      queryClient.setQueryData<Task[]>(queryKey, (current) =>
        current?.map((task) => (task.id === id ? { ...task, status } : task))
      );
      return { previous };
    },
    onError: (_error, _move, snapshot) => {
      queryClient.setQueryData<Task[]>(queryKey, snapshot?.previous);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['tasks'] });
    },
  });

  const deleteTaskMutation = useMutation({
    mutationFn: (id: string) => tasksApi.deleteTask(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['tasks'] });
    },
  });

  return {
    tasks,
    loading: workspaceId ? isLoading : workspaceLoading,
    error: error instanceof Error ? error.message : error ? 'Failed to load tasks' : null,
    createTask: createTaskMutation.mutateAsync,
    updateTask: (id: string, request: UpdateTaskRequest) =>
      updateTaskMutation.mutateAsync({ id, request }),
    moveTask: (id: string, status: TaskStatus) => moveTaskMutation.mutateAsync({ id, status }),
    deleteTask: deleteTaskMutation.mutateAsync,
    refetch,
  };
}
