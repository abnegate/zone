import { useCallback, useEffect, useState } from 'react';
import { projectsApi } from '../../../api/projects';
import type { Project, CreateProjectRequest, UpdateProjectRequest, ProjectStatus } from '../types';

export function useProjects(statusFilter?: ProjectStatus | 'all') {
  const [projects, setProjects] = useState<Project[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchProjects = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const status = statusFilter && statusFilter !== 'all' ? statusFilter : undefined;
      const data = await projectsApi.getProjects(status);
      setProjects(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load projects');
      setProjects([]);
    } finally {
      setLoading(false);
    }
  }, [statusFilter]);

  useEffect(() => {
    fetchProjects();
  }, [fetchProjects]);

  const createProject = useCallback(async (request: CreateProjectRequest): Promise<Project> => {
    const project = await projectsApi.createProject(request);
    setProjects((prev) => [project, ...prev]);
    return project;
  }, []);

  const updateProject = useCallback(
    async (id: string, request: UpdateProjectRequest): Promise<Project> => {
      const updated = await projectsApi.updateProject(id, request);
      setProjects((prev) => prev.map((p) => (p.id === updated.id ? updated : p)));
      return updated;
    },
    []
  );

  const deleteProject = useCallback(async (id: string): Promise<void> => {
    await projectsApi.deleteProject(id);
    setProjects((prev) => prev.filter((p) => p.id !== id));
  }, []);

  const refetch = useCallback(() => {
    return fetchProjects();
  }, [fetchProjects]);

  return {
    projects,
    loading,
    error,
    createProject,
    updateProject,
    deleteProject,
    refetch,
  };
}
