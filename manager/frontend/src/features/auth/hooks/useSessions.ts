import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { client } from '../../../api/client';

export function useSessions() {
  const queryClient = useQueryClient();
  const queryKey = ['sessions'];

  const { data, isLoading, error } = useQuery({
    queryKey,
    queryFn: () => client.getSessions(),
  });

  const revokeSessionMutation = useMutation({
    mutationFn: (sessionId: string) => client.revokeSession(sessionId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey });
    },
  });

  const revokeAllSessionsMutation = useMutation({
    mutationFn: () => client.revokeAllSessions(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey });
    },
  });

  return {
    sessions: data?.sessions || [],
    isLoading,
    error: error instanceof Error ? error.message : error ? 'Failed to load sessions' : null,
    revokeSession: revokeSessionMutation.mutateAsync,
    isRevoking: revokeSessionMutation.isPending,
    revokeAllSessions: revokeAllSessionsMutation.mutateAsync,
    isRevokingAll: revokeAllSessionsMutation.isPending,
  };
}
