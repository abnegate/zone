import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { mcpApi } from '../../../../api/mcp';
import { mcpQueryKeys } from '../mcpQueryKeys';
import type { CreateMcpServerRequest, McpServer, UpdateMcpServerRequest } from '../types';
import { pollInterval } from './pollInterval';

const NO_SERVERS: McpServer[] = [];

export interface McpImportResult {
  created: McpServer[];
  failures: { name: string; message: string }[];
}

function reason(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function useMcpServers(organizationId: string) {
  const queryClient = useQueryClient();

  const { data, isLoading, isFetching, error, refetch } = useQuery({
    queryKey: mcpQueryKeys.servers(organizationId),
    queryFn: () => mcpApi.list(organizationId),
    refetchInterval: (query) => pollInterval(query.state.data?.servers),
  });

  const invalidate = useCallback(
    () => queryClient.invalidateQueries({ queryKey: mcpQueryKeys.organization(organizationId) }),
    [queryClient, organizationId]
  );

  const refresh = useCallback(async (): Promise<void> => {
    await refetch();
  }, [refetch]);

  const create = useCallback(
    async (request: CreateMcpServerRequest): Promise<McpServer> => {
      const server = await mcpApi.create(organizationId, request);
      await invalidate();
      return server;
    },
    [organizationId, invalidate]
  );

  const update = useCallback(
    async (serverId: string, request: UpdateMcpServerRequest): Promise<McpServer> => {
      const server = await mcpApi.update(organizationId, serverId, request);
      await invalidate();
      return server;
    },
    [organizationId, invalidate]
  );

  const remove = useCallback(
    async (serverId: string): Promise<void> => {
      await mcpApi.remove(organizationId, serverId);
      await invalidate();
    },
    [organizationId, invalidate]
  );

  const reconnect = useCallback(
    async (serverId: string): Promise<McpServer> => {
      const server = await mcpApi.reconnect(organizationId, serverId);
      await invalidate();
      return server;
    },
    [organizationId, invalidate]
  );

  const importAll = useCallback(
    async (requests: CreateMcpServerRequest[]): Promise<McpImportResult> => {
      const result: McpImportResult = { created: [], failures: [] };
      for (const request of requests) {
        try {
          result.created.push(await mcpApi.create(organizationId, request));
        } catch (failure) {
          result.failures.push({ name: request.name, message: reason(failure) });
        }
      }
      await invalidate();
      return result;
    },
    [organizationId, invalidate]
  );

  return {
    servers: data?.servers ?? NO_SERVERS,
    commands: data?.commands ?? false,
    loading: isLoading,
    fetching: isFetching,
    error: error ? error.message : null,
    refresh,
    create,
    update,
    remove,
    reconnect,
    importAll,
  };
}
