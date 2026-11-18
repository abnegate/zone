import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { ENTITLED, ENTRA, OKTA } from '../fixtures';
import { ssoQueryKeys } from '../queryKeys';
import { SsoRefusal } from '../SsoRefusal';
import { SsoAdminCode } from '../schemas';
import type { SettingsResponse } from '../types';

const deleteProvider = mock();

mock.module('../api', () => ({
  ssoApi: {
    get: mock(),
    createProvider: mock(),
    updateProvider: mock(),
    deleteProvider,
    setEnforcement: mock(),
  },
}));

let useDeleteSsoProvider: typeof import('./useDeleteSsoProvider').useDeleteSsoProvider;

beforeAll(async () => {
  ({ useDeleteSsoProvider } = await import('./useDeleteSsoProvider'));
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
  return { queryClient, ...renderHook(() => useDeleteSsoProvider(), { wrapper }) };
}

describe('useDeleteSsoProvider', () => {
  beforeEach(() => {
    deleteProvider.mockReset();
  });

  it('drops the provider from the cache and reports it deleted', async () => {
    deleteProvider.mockResolvedValue(undefined);
    const deleted = mock();
    const { queryClient, result } = setup();

    act(() => result.current.remove(OKTA.id, deleted));

    await waitFor(() => expect(deleted).toHaveBeenCalledTimes(1));
    expect(deleteProvider).toHaveBeenCalledWith(OKTA.id);
    expect(queryClient.getQueryData<SettingsResponse>(ssoQueryKeys.settings)?.providers).toEqual([
      ENTRA,
    ]);
    expect(queryClient.getQueryState(ssoQueryKeys.settings)?.isInvalidated).toBe(true);
  });

  it('exposes a refusal and keeps the provider', async () => {
    deleteProvider.mockRejectedValue(
      new SsoRefusal({ error: 'Still required', code: SsoAdminCode.ProviderRequired }, 409)
    );
    const deleted = mock();
    const { queryClient, result } = setup();

    act(() => result.current.remove(OKTA.id, deleted));

    await waitFor(() => expect(result.current.refusal?.code).toBe(SsoAdminCode.ProviderRequired));
    expect(deleted).not.toHaveBeenCalled();
    expect(result.current.pending).toBe(false);
    expect(queryClient.getQueryData<SettingsResponse>(ssoQueryKeys.settings)?.providers).toEqual([
      OKTA,
      ENTRA,
    ]);
  });
});
