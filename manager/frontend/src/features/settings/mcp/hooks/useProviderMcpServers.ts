import { useQuery } from '@tanstack/react-query';
import { mcpApi } from '../../../../api/mcp';
import { mcpQueryKeys } from '../queryKeys';
import type { ProviderMcpServers } from '../types';

const NO_PROVIDERS: ProviderMcpServers[] = [];

export interface ProviderMcpServersState {
  providers: ProviderMcpServers[];
  commands: boolean;
  loaded: boolean;
  loading: boolean;
  error: string | null;
}

/**
 * What each provider the organization is connected to has configured, read only while the import
 * dialog is open: it reads files on the server's host, so nothing asks for it in the background.
 */
export function useProviderMcpServers(
  organizationId: string,
  enabled = true
): ProviderMcpServersState {
  const { data, isLoading, error } = useQuery({
    queryKey: mcpQueryKeys.providers(organizationId),
    queryFn: () => mcpApi.providers(organizationId),
    enabled,
    staleTime: 0,
    gcTime: 0,
  });

  return {
    providers: data?.providers ?? NO_PROVIDERS,
    commands: data?.commands ?? false,
    loaded: data !== undefined,
    loading: isLoading,
    error: error ? error.message : null,
  };
}
