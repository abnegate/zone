import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { ENTITLED, ENTRA, OKTA } from '../fixtures';
import { ssoQueryKeys } from '../queryKeys';
import { SsoRefusal } from '../SsoRefusal';
import { SsoAdminCode } from '../schemas';
import type { SettingsResponse } from '../types';

const updateProvider = mock();

mock.module('../api', () => ({
  ssoApi: {
    get: mock(),
    createProvider: mock(),
    updateProvider,
    deleteProvider: mock(),
    setEnforcement: mock(),
  },
}));

let useUpdateSsoProvider: typeof import('./useUpdateSsoProvider').useUpdateSsoProvider;

beforeAll(async () => {
  ({ useUpdateSsoProvider } = await import('./useUpdateSsoProvider'));
});

afterAll(() => {
  mock.restore();
});

function setup() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity }, mutations: { retry: false } },
  });
  queryClient.setQueryData(ssoQueryKeys.settings, ENTITLED);
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
  return { queryClient, ...renderHook(() => useUpdateSsoProvider(), { wrapper }) };
}

describe('useUpdateSsoProvider', () => {
  beforeEach(() => {
    updateProvider.mockReset();
  });

  it('sends the sparse update and swaps the provider in the cache', async () => {
    const renamed = { ...OKTA, display_name: 'Okta SSO' };
    updateProvider.mockResolvedValue(renamed);
    const updated = mock();
    const { queryClient, result } = setup();

    act(() => result.current.update(OKTA.id, { display_name: 'Okta SSO' }, updated));

    await waitFor(() => expect(updated).toHaveBeenCalledWith(renamed));
    expect(updateProvider).toHaveBeenCalledWith(OKTA.id, { display_name: 'Okta SSO' });
    expect(queryClient.getQueryData<SettingsResponse>(ssoQueryKeys.settings)?.providers).toEqual([
      renamed,
      ENTRA,
    ]);
    expect(queryClient.getQueryState(ssoQueryKeys.settings)?.isInvalidated).toBe(true);
  });

  it('exposes a refusal and keeps the cached provider', async () => {
    updateProvider.mockRejectedValue(
      new SsoRefusal({ error: 'Still required', code: SsoAdminCode.ProviderRequired }, 409)
    );
    const { queryClient, result } = setup();

    act(() => result.current.update(OKTA.id, { enabled: false }));

    await waitFor(() => expect(result.current.refusal?.code).toBe(SsoAdminCode.ProviderRequired));
    expect(result.current.refusal?.status).toBe(409);
    expect(queryClient.getQueryData<SettingsResponse>(ssoQueryKeys.settings)?.providers).toEqual([
      OKTA,
      ENTRA,
    ]);
  });
});
