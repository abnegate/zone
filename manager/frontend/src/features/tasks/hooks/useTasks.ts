import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { tasksApi } from '../../../api/tasks';
import type { CreateTaskRequest, UpdateTaskRequest } from '../types';

export function useTasks(projectId?: string, status?: string) {
  const queryClient = useQueryClient();
  const queryKey = ['tasks', projectId, status];

  const { data: tasks = [], isLoading: loading, error, refetch } = useQuery({
    queryKey,
    queryFn: () => tasksApi.getTasks(projectId, status),
  });

  const createTaskMutation = useMutation({
    mutationFn: (request: CreateTaskRequest) => tasksApi.createTask(request),
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
