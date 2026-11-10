import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { mcpApi } from '../../../../api/mcp';
import { mcpQueryKeys } from '../mcpQueryKeys';
import type { WorkspaceMcpServer } from '../types';
import { pollInterval } from './pollInterval';

const NO_SERVERS: WorkspaceMcpServer[] = [];

export function useWorkspaceMcpServers(organizationId: string, workspaceId: string) {
  const queryClient = useQueryClient();

  const { data, isLoading, isFetching, error, refetch } = useQuery({
    queryKey: mcpQueryKeys.workspace(organizationId, workspaceId),
    queryFn: () => mcpApi.workspace(organizationId, workspaceId),
    refetchInterval: (query) => pollInterval(query.state.data?.servers),
  });

  const refresh = useCallback(async (): Promise<void> => {
    await refetch();
  }, [refetch]);

  const update = useCallback(
    async (changes: Record<string, boolean | null>): Promise<void> => {
      const list = await mcpApi.updateWorkspace(organizationId, workspaceId, { servers: changes });
      queryClient.setQueryData(mcpQueryKeys.workspace(organizationId, workspaceId), list);
      await queryClient.invalidateQueries({ queryKey: mcpQueryKeys.servers(organizationId) });
    },
    [queryClient, organizationId, workspaceId]
  );

  return {
    servers: data?.servers ?? NO_SERVERS,
    loading: isLoading,
    fetching: isFetching,
    error: error ? error.message : null,
    refresh,
    update,
  };
}
