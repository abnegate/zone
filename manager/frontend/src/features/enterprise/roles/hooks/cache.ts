import type { QueryClient } from '@tanstack/react-query';
import { rolesQueryKeys } from '../queryKeys';
import type { CustomRole, RolesResponse } from '../types';

export function cacheRoles(
  queryClient: QueryClient,
  organizationId: string,
  change: (roles: readonly CustomRole[]) => CustomRole[]
): void {
  queryClient.setQueryData<RolesResponse>(
    rolesQueryKeys.organization(organizationId),
    (response) => (response ? { ...response, roles: change(response.roles) } : response)
  );
}

export function refreshRoles(queryClient: QueryClient, organizationId: string): Promise<void> {
  return queryClient.invalidateQueries({ queryKey: rolesQueryKeys.organization(organizationId) });
}
