import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { ApiError } from '../../../../api/ApiError';
import { HttpStatus } from '../../../../api/HttpStatus';
import { COMMUNITY_EMPTY, ENTITLED } from '../fixtures';

const get = mock();

mock.module('../api', () => ({
  ssoApi: {
    get,
    createProvider: mock(),
    updateProvider: mock(),
    deleteProvider: mock(),
    setEnforcement: mock(),
  },
}));

let useSso: typeof import('./useSso').useSso;
let SSO_RETRY_LIMIT: number;

beforeAll(async () => {
  ({ useSso, SSO_RETRY_LIMIT } = await import('./useSso'));
});

afterAll(() => {
  mock.restore();
});

function render() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retryDelay: 0, gcTime: 0 } },
  });
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
  return renderHook(() => useSso(), { wrapper });
}

describe('useSso', () => {
  beforeEach(() => {
    get.mockReset();
  });

  it('is loading until the settings arrive', async () => {
    get.mockResolvedValue(ENTITLED);
    const { result } = render();

    expect(result.current.loading).toBe(true);
    expect(result.current.settings).toBeUndefined();
    await waitFor(() => expect(result.current.settings).toEqual(ENTITLED));
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
  });

  for (const status of [HttpStatus.Forbidden, HttpStatus.NotFound, HttpStatus.TooManyRequests]) {
    it(`reports a ${status} at once without asking again`, async () => {
      get.mockRejectedValue(new ApiError(`Failed to load single sign-on: ${status}`, status));
      const { result } = render();

      await waitFor(() =>
        expect(result.current.error).toBe(`Failed to load single sign-on: ${status}`)
      );
      expect(get).toHaveBeenCalledTimes(1);
    });
  }

  it('retries a server failure and shows the settings once they load', async () => {
    get
      .mockRejectedValueOnce(new ApiError('Failed to load single sign-on: 500', 500))
      .mockResolvedValueOnce(ENTITLED);
    const { result } = render();

    await waitFor(() => expect(result.current.settings).toEqual(ENTITLED));
    expect(get).toHaveBeenCalledTimes(2);
  });

  it('gives up after the retry limit', async () => {
    get.mockRejectedValue(new ApiError('Failed to load single sign-on: 502', 502));
    const { result } = render();

    await waitFor(() => expect(result.current.error).toBe('Failed to load single sign-on: 502'));
    expect(get).toHaveBeenCalledTimes(SSO_RETRY_LIMIT + 1);
  });

  it('retries a network failure that never reached the server', async () => {
    get.mockRejectedValueOnce(new TypeError('Failed to fetch')).mockResolvedValueOnce(ENTITLED);
    const { result } = render();

    await waitFor(() => expect(result.current.settings).toEqual(ENTITLED));
    expect(get).toHaveBeenCalledTimes(2);
  });

  it('loads again on refresh', async () => {
    get.mockResolvedValueOnce(COMMUNITY_EMPTY).mockResolvedValueOnce(ENTITLED);
    const { result } = render();
    await waitFor(() => expect(result.current.settings).toEqual(COMMUNITY_EMPTY));

    await act(() => result.current.refresh());

    await waitFor(() => expect(result.current.settings).toEqual(ENTITLED));
  });
});
