import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { projectsApi } from '../../../api/projects';
import type { CreateProjectRequest, UpdateProjectRequest, ProjectStatus } from '../types';

export function useProjects(statusFilter?: ProjectStatus | 'all') {
  const queryClient = useQueryClient();
  const queryKey = ['projects', statusFilter];

  const { data: projects = [], isLoading: loading, error, refetch } = useQuery({
    queryKey,
    queryFn: () => {
      const status = statusFilter && statusFilter !== 'all' ? statusFilter : undefined;
      return projectsApi.getProjects(status);
    }
  });

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
