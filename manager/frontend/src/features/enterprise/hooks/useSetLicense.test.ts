import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { ApiError } from '../../../api/ApiError';
import { ACTIVE, COMMUNITY } from '../fixtures';
import { LicenseRefusal } from '../LicenseRefusal';
import { licenseQueryKeys } from '../queryKeys';
import { LicenseRefusalCode } from '../schemas';
import type { LicenseResponse } from '../types';

const get = mock();
const set = mock();
const remove = mock();

mock.module('../api', () => ({ licenseApi: { get, set, remove } }));

let useSetLicense: typeof import('./useSetLicense').useSetLicense;

beforeAll(async () => {
  ({ useSetLicense } = await import('./useSetLicense'));
});

afterAll(() => {
  mock.restore();
});

function setup() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity }, mutations: { retry: false } },
  });
  queryClient.setQueryData(licenseQueryKeys.current, COMMUNITY);
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
  return { queryClient, ...renderHook(() => useSetLicense(), { wrapper }) };
}

describe('useSetLicense', () => {
  beforeEach(() => {
    get.mockReset();
    set.mockReset();
    remove.mockReset();
  });

  it('writes the new license into the cache and reports it saved', async () => {
    set.mockResolvedValue(ACTIVE);
    const saved = mock();
    const { queryClient, result } = setup();

    act(() => result.current.set('payload.signature', saved));

    await waitFor(() => expect(saved).toHaveBeenCalled());
    expect(set).toHaveBeenCalledWith('payload.signature');
    expect(saved.mock.calls[0][0]).toEqual(ACTIVE);
    expect(queryClient.getQueryData<LicenseResponse>(licenseQueryKeys.current)).toEqual(ACTIVE);
    expect(get).not.toHaveBeenCalled();
  });

  it('keeps the new license when a license read in flight settles after the save', async () => {
    let settleRead: (value: LicenseResponse) => void = () => {};
    get.mockImplementation(() => new Promise((settle) => (settleRead = settle)));
    set.mockResolvedValue(ACTIVE);
    const saved = mock();
    const { queryClient, result } = setup();
    const read = queryClient
      .fetchQuery({ queryKey: licenseQueryKeys.current, queryFn: get })
      .catch(() => undefined);
    await waitFor(() => expect(get).toHaveBeenCalledTimes(1));

    act(() => result.current.set('payload.signature', saved));
    await waitFor(() => expect(saved).toHaveBeenCalled());
    settleRead(COMMUNITY);
    await read;

    expect(queryClient.getQueryData<LicenseResponse>(licenseQueryKeys.current)).toEqual(ACTIVE);
  });

  it('is pending while the key is checked', async () => {
    let resolve: (value: unknown) => void = () => {};
    set.mockImplementation(() => new Promise((settle) => (resolve = settle)));
    const { result } = setup();

    act(() => result.current.set('payload.signature'));

    await waitFor(() => expect(result.current.pending).toBe(true));
    await act(async () => resolve(ACTIVE));
    await waitFor(() => expect(result.current.pending).toBe(false));
  });

  it('exposes a refusal and leaves the cache alone', async () => {
    set.mockRejectedValue(
      new LicenseRefusal({
        error: 'Signature check failed',
        code: LicenseRefusalCode.SignatureInvalid,
      })
    );
    const saved = mock();
    const { queryClient, result } = setup();

    act(() => result.current.set('payload.signature', saved));

    await waitFor(() => expect(result.current.refusal).not.toBeNull());
    expect(result.current.refusal?.code).toBe(LicenseRefusalCode.SignatureInvalid);
    expect(result.current.error).toBe('Signature check failed');
    expect(saved).not.toHaveBeenCalled();
    expect(queryClient.getQueryData<LicenseResponse>(licenseQueryKeys.current)).toEqual(COMMUNITY);
  });

  it('exposes any other failure without a refusal', async () => {
    set.mockRejectedValue(
      new ApiError('Failed to save the license: This needs a server admin.', 403)
    );
    const { result } = setup();

    act(() => result.current.set('payload.signature'));

    await waitFor(() => expect(result.current.error).not.toBeNull());
    expect(result.current.refusal).toBeNull();
    expect(result.current.error).toBe('Failed to save the license: This needs a server admin.');
  });

  it('clears a failure on reset', async () => {
    set.mockRejectedValue(new Error('Network down'));
    const { result } = setup();

    act(() => result.current.set('payload.signature'));
    await waitFor(() => expect(result.current.error).toBe('Network down'));

    act(() => result.current.reset());

    await waitFor(() => expect(result.current.error).toBeNull());
  });
});
