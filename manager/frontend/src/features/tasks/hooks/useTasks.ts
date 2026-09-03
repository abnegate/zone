import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { tasksApi } from '../../../api/tasks';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { CreateTaskRequest, UpdateTaskRequest } from '../types';

export function useTasks(projectId?: string, status?: string) {
  const queryClient = useQueryClient();
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const queryKey = ['tasks', workspaceId, projectId, status];

  const {
    data: tasks = [],
    isLoading: loading,
    error,
    refetch,
  } = useQuery({
    queryKey,
    queryFn: () => {
      if (!workspaceId) {
        return [];
      }
      return tasksApi.getTasks(workspaceId, projectId, status);
    },
    enabled: !!workspaceId,
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
    onSuccess: () => {
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
    loading,
    error: error instanceof Error ? error.message : error ? 'Failed to load tasks' : null,
    createTask: createTaskMutation.mutateAsync,
    updateTask: (id: string, request: UpdateTaskRequest) =>
      updateTaskMutation.mutateAsync({ id, request }),
    deleteTask: deleteTaskMutation.mutateAsync,
    refetch,
  };
}
