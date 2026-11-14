import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { mcpApi } from '../../../../api/mcp';
import { messageOf } from '../failure';
import { mcpQueryKeys } from '../queryKeys';
import type {
  CreateMcpServerRequest,
  McpServer,
  McpServerList,
  UpdateMcpServerRequest,
} from '../types';
import { pollInterval } from './pollInterval';

const NO_SERVERS: McpServer[] = [];

const IMPORT_FAILED = 'Failed to add the MCP server';
const PROVIDER_IMPORT_FAILED = 'Failed to import the MCP servers';

export interface McpImportResult {
  created: McpServer[];
  failures: { name: string; message: string }[];
}

/** The servers chosen from one connected provider, by the names that provider configured. */
export interface McpProviderSelection {
  provider: string;
  label: string;
  servers: string[];
}

export interface McpServersState {
  servers: McpServer[];
  commands: boolean;
  loaded: boolean;
  loading: boolean;
  fetching: boolean;
  error: string | null;
  refresh(): Promise<void>;
  create(request: CreateMcpServerRequest): Promise<McpServer>;
  update(serverId: string, request: UpdateMcpServerRequest): Promise<McpServer>;
  remove(serverId: string): Promise<void>;
  reconnect(serverId: string): Promise<McpServer>;
  importAll(requests: CreateMcpServerRequest[]): Promise<McpImportResult>;
  importProviders(selections: McpProviderSelection[]): Promise<McpImportResult>;
}

function replaced(
  current: McpServerList | undefined,
  server: McpServer
): McpServerList | undefined {
  return (
    current && {
      ...current,
      servers: current.servers.map((entry) => (entry.id === server.id ? server : entry)),
    }
  );
}

export function useMcpServers(organizationId: string): McpServersState {
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

  const settle = useCallback(
    async (server: McpServer): Promise<McpServer> => {
      queryClient.setQueryData<McpServerList>(mcpQueryKeys.servers(organizationId), (current) =>
        replaced(current, server)
      );
      await invalidate();
      return server;
    },
    [queryClient, organizationId, invalidate]
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
    async (serverId: string, request: UpdateMcpServerRequest): Promise<McpServer> =>
      settle(await mcpApi.update(organizationId, serverId, request)),
    [organizationId, settle]
  );

  const remove = useCallback(
    async (serverId: string): Promise<void> => {
      await mcpApi.remove(organizationId, serverId);
      await invalidate();
    },
    [organizationId, invalidate]
  );

  const reconnect = useCallback(
    async (serverId: string): Promise<McpServer> =>
      settle(await mcpApi.reconnect(organizationId, serverId)),
    [organizationId, settle]
  );

  const importAll = useCallback(
    async (requests: CreateMcpServerRequest[]): Promise<McpImportResult> => {
      const result: McpImportResult = { created: [], failures: [] };
      for (const request of requests) {
        try {
          result.created.push(await mcpApi.create(organizationId, request));
        } catch (failure) {
          result.failures.push({ name: request.name, message: messageOf(failure, IMPORT_FAILED) });
        }
      }
      await invalidate();
      return result;
    },
    [organizationId, invalidate]
  );

  const importProviders = useCallback(
    async (selections: McpProviderSelection[]): Promise<McpImportResult> => {
      const result: McpImportResult = { created: [], failures: [] };
      for (const selection of selections) {
        if (selection.servers.length === 0) {
          continue;
        }
        try {
          const imported = await mcpApi.import(organizationId, {
            provider: selection.provider,
            servers: selection.servers,
          });
          result.created.push(...imported.created);
          result.failures.push(...imported.failures);
        } catch (failure) {
          const message = messageOf(failure, PROVIDER_IMPORT_FAILED);
          result.failures.push(
            ...selection.servers.map((name) => ({ name: `${selection.label}: ${name}`, message }))
          );
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
    loaded: data !== undefined,
    loading: isLoading,
    fetching: isFetching,
    error: error ? error.message : null,
    refresh,
    create,
    update,
    remove,
    reconnect,
    importAll,
    importProviders,
  };
}
