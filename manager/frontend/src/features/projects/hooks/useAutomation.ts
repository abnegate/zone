import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { projectsApi } from '../../../api/projects';

/** How often the panel re-reads a project that is running itself. */
const LIVE_REFETCH_MS = 15_000;

/**
 * What automation knows about one project, re-read while it runs.
 *
 * Polling rather than a socket: the page is the only reader, the data is a
 * handful of rows, and fifteen seconds is one driver tick.
 */
export function useAutomation(projectId: string | null, live: boolean) {
  const queryClient = useQueryClient();
  const queryKey = ['project-automation', projectId];

  const query = useQuery({
    queryKey,
    queryFn: () => projectsApi.getAutomation(projectId as string),
    enabled: !!projectId,
    refetchInterval: live ? LIVE_REFETCH_MS : false,
  });

  const resumeMutation = useMutation({
    mutationFn: () => projectsApi.resumeAutomation(projectId as string),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey });
      queryClient.invalidateQueries({ queryKey: ['projects'] });
    },
  });

  return {
    automation: query.data ?? null,
    loading: !!projectId && query.isLoading,
    error: query.error instanceof Error ? query.error.message : null,
    refetch: query.refetch,
    resume: resumeMutation.mutateAsync,
    resuming: resumeMutation.isPending,
  };
}
