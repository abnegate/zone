import { useCallback, useEffect, useState } from 'react';
import { projectsApi } from '../../../api/projects';
import type { Project } from '../types';

export function useProject(id: string | null) {
  const [project, setProject] = useState<Project | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchProject = useCallback(async () => {
    if (!id) {
      setProject(null);
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);
    try {
      const data = await projectsApi.getProject(id);
      setProject(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load project');
      setProject(null);
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    fetchProject();
  }, [fetchProject]);

  const refetch = useCallback(() => {
    return fetchProject();
  }, [fetchProject]);

  return {
    project,
    loading,
    error,
    refetch,
  };
}
