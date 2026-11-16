import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { ACTIVE, COMMUNITY } from '../fixtures';
import { licenseQueryKeys } from '../queryKeys';
import type { LicenseResponse } from '../types';

const get = mock();
const set = mock();
const remove = mock();

mock.module('../api', () => ({ licenseApi: { get, set, remove } }));

let useRemoveLicense: typeof import('./useRemoveLicense').useRemoveLicense;

beforeAll(async () => {
  ({ useRemoveLicense } = await import('./useRemoveLicense'));
});

afterAll(() => {
  mock.restore();
});

function setup() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity }, mutations: { retry: false } },
  });
  queryClient.setQueryData(licenseQueryKeys.current, ACTIVE);
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
  return { queryClient, ...renderHook(() => useRemoveLicense(), { wrapper }) };
}

describe('useRemoveLicense', () => {
  beforeEach(() => {
    get.mockReset();
    set.mockReset();
    remove.mockReset();
  });

  it('writes the Community response into the cache and reports it removed', async () => {
    remove.mockResolvedValue(COMMUNITY);
    const removed = mock();
    const { queryClient, result } = setup();

    act(() => result.current.remove(removed));

    await waitFor(() => expect(removed).toHaveBeenCalled());
    expect(remove).toHaveBeenCalledTimes(1);
    expect(queryClient.getQueryData<LicenseResponse>(licenseQueryKeys.current)).toEqual(COMMUNITY);
    expect(get).not.toHaveBeenCalled();
  });

  it('exposes a failure and keeps the license in the cache', async () => {
    remove.mockRejectedValue(new Error('Failed to remove the license: 500'));
    const removed = mock();
    const { queryClient, result } = setup();

    act(() => result.current.remove(removed));

    await waitFor(() => expect(result.current.error).toBe('Failed to remove the license: 500'));
    expect(removed).not.toHaveBeenCalled();
    expect(result.current.pending).toBe(false);
    expect(queryClient.getQueryData<LicenseResponse>(licenseQueryKeys.current)).toEqual(ACTIVE);
  });
});
