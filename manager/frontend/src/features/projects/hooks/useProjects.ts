import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { projectsApi } from '../../../api/projects';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { CreateProjectRequest, UpdateProjectRequest, ProjectStatus } from '../types';

export function useProjects(statusFilter?: ProjectStatus | 'all') {
  const queryClient = useQueryClient();
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const queryKey = ['projects', workspaceId, statusFilter];

  const { data: projects = [], isLoading, isFetching, error, refetch } = useQuery({
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

  // Only show loading when we have a workspace and are actually fetching
  const loading = !!workspaceId && (isLoading || isFetching);

  const createProjectMutation = useMutation({
    mutationFn: (request: CreateProjectRequest) => projectsApi.createProject(request),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
    }
  });

  const updateProjectMutation = useMutation({
    mutationFn: ({ id, request }: { id: string; request: UpdateProjectRequest }) =>
      projectsApi.updateProject(id, request),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
    }
  });

  const deleteProjectMutation = useMutation({
    mutationFn: (id: string) => projectsApi.deleteProject(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
    }
  });

  return {
    projects,
    loading,
    error: error instanceof Error ? error.message : error ? 'Failed to load projects' : null,
    createProject: createProjectMutation.mutateAsync,
    updateProject: (id: string, request: UpdateProjectRequest) => 
        updateProjectMutation.mutateAsync({ id, request }),
    deleteProject: deleteProjectMutation.mutateAsync,
    refetch,
  };
}
