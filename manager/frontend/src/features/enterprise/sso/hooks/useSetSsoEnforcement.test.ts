import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { ENTITLED } from '../fixtures';
import { ssoQueryKeys } from '../queryKeys';
import { SsoRefusal } from '../SsoRefusal';
import { SsoAdminCode } from '../schemas';
import type { SettingsResponse } from '../types';

const setEnforcement = mock();

mock.module('../api', () => ({
  ssoApi: {
    get: mock(),
    createProvider: mock(),
    updateProvider: mock(),
    deleteProvider: mock(),
    setEnforcement,
  },
}));

let useSetSsoEnforcement: typeof import('./useSetSsoEnforcement').useSetSsoEnforcement;

beforeAll(async () => {
  ({ useSetSsoEnforcement } = await import('./useSetSsoEnforcement'));
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
  return { queryClient, ...renderHook(() => useSetSsoEnforcement(), { wrapper }) };
}

describe('useSetSsoEnforcement', () => {
  beforeEach(() => {
    setEnforcement.mockReset();
  });

  it('writes the new requirement into the cache and reports it', async () => {
    setEnforcement.mockResolvedValue({ required: true });
    const changed = mock();
    const { queryClient, result } = setup();

    act(() => result.current.set(true, changed));

    await waitFor(() => expect(changed).toHaveBeenCalledWith({ required: true }));
    expect(setEnforcement).toHaveBeenCalledWith(true);
    expect(queryClient.getQueryData<SettingsResponse>(ssoQueryKeys.settings)?.required).toBe(true);
    expect(queryClient.getQueryState(ssoQueryKeys.settings)?.isInvalidated).toBe(true);
  });

  it('exposes a refusal and leaves the requirement off', async () => {
    setEnforcement.mockRejectedValue(
      new SsoRefusal({ error: 'No provider', code: SsoAdminCode.ProviderRequired }, 409)
    );
    const { queryClient, result } = setup();

    act(() => result.current.set(true));

    await waitFor(() => expect(result.current.refusal?.code).toBe(SsoAdminCode.ProviderRequired));
    expect(queryClient.getQueryData<SettingsResponse>(ssoQueryKeys.settings)?.required).toBe(false);

    act(() => result.current.reset());

    await waitFor(() => expect(result.current.refusal).toBeNull());
  });
});
