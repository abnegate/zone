import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { projectsApi } from '../../../api/projects';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { CreateProjectRequest, Project, ProjectStatus, UpdateProjectRequest } from '../types';
import { applyOrder } from '../utils/order';

export function useProjects(statusFilter?: ProjectStatus | 'all') {
  const queryClient = useQueryClient();
  const { currentWorkspace, loading: workspaceLoading } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const queryKey = ['projects', workspaceId, statusFilter];

  const {
    data: projects = [],
    isLoading,
    isFetching,
    error,
    refetch,
  } = useQuery({
    queryKey,
    queryFn: () => {
      if (!workspaceId) {
        return [];
      }
      const status = statusFilter && statusFilter !== 'all' ? statusFilter : undefined;
      return projectsApi.getProjects(workspaceId, status);
    },
    enabled: !!workspaceId,
  });

  const loading = workspaceId ? isLoading || isFetching : workspaceLoading;

  const createProjectMutation = useMutation({
    mutationFn: (request: CreateProjectRequest) => projectsApi.createProject(request),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
    },
  });

  const updateProjectMutation = useMutation({
    mutationFn: ({ id, request }: { id: string; request: UpdateProjectRequest }) =>
      projectsApi.updateProject(id, request),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
    },
  });

  const reorderProjectsMutation = useMutation({
    scope: { id: 'project-order' },
    mutationFn: (order: readonly string[]) => {
      if (!workspaceId) {
        return Promise.reject(new Error('No workspace selected'));
      }
      return projectsApi.reorderProjects(workspaceId, order);
    },
    onMutate: async (order) => {
      const listed = { queryKey: ['projects', workspaceId] };
      await queryClient.cancelQueries(listed);
      const previous = queryClient.getQueriesData<Project[]>(listed);
      queryClient.setQueriesData<Project[]>(listed, (current) =>
        current ? applyOrder(current, order) : current
      );
      return { previous };
    },
    onError: (_error, _order, context) => {
      for (const [key, data] of context?.previous ?? []) {
        queryClient.setQueryData(key, data);
      }
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['projects'], refetchType: 'inactive' });
    },
  });

  const deleteProjectMutation = useMutation({
    mutationFn: (id: string) => projectsApi.deleteProject(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
    },
  });

  return {
    projects,
    loading,
    error: error instanceof Error ? error.message : error ? 'Failed to load projects' : null,
    createProject: createProjectMutation.mutateAsync,
    updateProject: (id: string, request: UpdateProjectRequest) =>
      updateProjectMutation.mutateAsync({ id, request }),
    deleteProject: deleteProjectMutation.mutateAsync,
    reorderProjects: reorderProjectsMutation.mutateAsync,
    refetch,
  };
}
