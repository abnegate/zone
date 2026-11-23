import type { QueryClient } from '@tanstack/react-query';
import { scimQueryKeys } from '../queryKeys';
import type { GroupResponse, ScimSettings, TokenResponse } from '../types';

function cacheSettings(
  queryClient: QueryClient,
  organizationId: string,
  change: (settings: ScimSettings) => ScimSettings
): void {
  queryClient.setQueryData<ScimSettings>(scimQueryKeys.organization(organizationId), (settings) =>
    settings ? change(settings) : settings
  );
}

export function cacheTokens(
  queryClient: QueryClient,
  organizationId: string,
  change: (tokens: readonly TokenResponse[]) => TokenResponse[]
): void {
  cacheSettings(queryClient, organizationId, (settings) => ({
    ...settings,
    tokens: change(settings.tokens),
  }));
}

export function cacheGroup(
  queryClient: QueryClient,
  organizationId: string,
  saved: GroupResponse
): void {
  cacheSettings(queryClient, organizationId, (settings) => ({
    ...settings,
    groups: settings.groups.map((group) => (group.id === saved.id ? saved : group)),
  }));
}

export function refreshScim(queryClient: QueryClient, organizationId: string): Promise<void> {
  return queryClient.invalidateQueries({ queryKey: scimQueryKeys.organization(organizationId) });
}
