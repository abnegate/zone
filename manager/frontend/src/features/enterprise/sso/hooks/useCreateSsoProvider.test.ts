import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { ApiError } from '../../../../api/ApiError';
import { ENTITLED_EMPTY, OKTA } from '../fixtures';
import { ssoQueryKeys } from '../queryKeys';
import { SsoRefusal } from '../SsoRefusal';
import { SsoAdminCode } from '../schemas';
import type { CreateProviderRequest, SettingsResponse } from '../types';

const get = mock();
const createProvider = mock();

mock.module('../api', () => ({
  ssoApi: {
    get,
    createProvider,
    updateProvider: mock(),
    deleteProvider: mock(),
    setEnforcement: mock(),
  },
}));

let useCreateSsoProvider: typeof import('./useCreateSsoProvider').useCreateSsoProvider;

beforeAll(async () => {
  ({ useCreateSsoProvider } = await import('./useCreateSsoProvider'));
});

afterAll(() => {
  mock.restore();
});

const REQUEST: CreateProviderRequest = {
  display_name: 'Okta',
  issuer: 'https://acme.okta.com',
  client_id: '0oa1b2c3d4',
  client_secret: 's3cret',
};

function setup() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity }, mutations: { retry: false } },
  });
  queryClient.setQueryData(ssoQueryKeys.settings, ENTITLED_EMPTY);
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
  return { queryClient, ...renderHook(() => useCreateSsoProvider(), { wrapper }) };
}

describe('useCreateSsoProvider', () => {
  beforeEach(() => {
    get.mockReset();
    createProvider.mockReset();
  });

  it('adds the provider to the cached settings and marks them stale', async () => {
    createProvider.mockResolvedValue(OKTA);
    const created = mock();
    const { queryClient, result } = setup();

    act(() => result.current.create(REQUEST, created));

    await waitFor(() => expect(created).toHaveBeenCalledWith(OKTA));
    expect(createProvider).toHaveBeenCalledWith(REQUEST);
    expect(queryClient.getQueryData<SettingsResponse>(ssoQueryKeys.settings)?.providers).toEqual([
      OKTA,
    ]);
    expect(queryClient.getQueryState(ssoQueryKeys.settings)?.isInvalidated).toBe(true);
  });

  it('exposes a refusal with its code and leaves the providers alone', async () => {
    createProvider.mockRejectedValue(
      new SsoRefusal({ error: 'Discovery failed', code: SsoAdminCode.DiscoveryFailed }, 400)
    );
    const created = mock();
    const { queryClient, result } = setup();

    act(() => result.current.create(REQUEST, created));

    await waitFor(() => expect(result.current.refusal?.code).toBe(SsoAdminCode.DiscoveryFailed));
    expect(result.current.error).toBe('Discovery failed');
    expect(created).not.toHaveBeenCalled();
    expect(queryClient.getQueryData<SettingsResponse>(ssoQueryKeys.settings)?.providers).toEqual(
      []
    );
    expect(queryClient.getQueryState(ssoQueryKeys.settings)?.isInvalidated).toBe(true);
  });

  it('exposes any other failure without a refusal and clears it on reset', async () => {
    createProvider.mockRejectedValue(new ApiError('Failed to add the provider: 500', 500));
    const { result } = setup();

    act(() => result.current.create(REQUEST));
    await waitFor(() => expect(result.current.error).toBe('Failed to add the provider: 500'));
    expect(result.current.refusal).toBeNull();

    act(() => result.current.reset());

    await waitFor(() => expect(result.current.error).toBeNull());
  });

  it('is pending while the provider is checked', async () => {
    let resolve: (value: unknown) => void = () => {};
    createProvider.mockImplementation(() => new Promise((settle) => (resolve = settle)));
    const { result } = setup();

    act(() => result.current.create(REQUEST));

    await waitFor(() => expect(result.current.pending).toBe(true));
    await act(async () => resolve(OKTA));
    await waitFor(() => expect(result.current.pending).toBe(false));
  });
});
