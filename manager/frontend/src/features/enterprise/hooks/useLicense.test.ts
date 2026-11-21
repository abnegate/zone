import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { ApiError } from '../../../api/ApiError';
import { HttpStatus } from '../../../api/HttpStatus';
import { ACTIVE, COMMUNITY, UNVERIFIED } from '../fixtures';
import { RETRY_LIMIT } from '../retry';

const get = mock();

mock.module('../api', () => ({ licenseApi: { get, set: mock(), remove: mock() } }));

let useLicense: typeof import('./useLicense').useLicense;

beforeAll(async () => {
  ({ useLicense } = await import('./useLicense'));
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
  return renderHook(() => useLicense(), { wrapper });
}

describe('useLicense', () => {
  beforeEach(() => {
    get.mockReset();
  });

  it('is loading until the license arrives', async () => {
    get.mockResolvedValue(COMMUNITY);
    const { result } = render();

    expect(result.current.loading).toBe(true);
    expect(result.current.response).toBeUndefined();
    await waitFor(() => expect(result.current.response).toEqual(COMMUNITY));
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it('reads a stored key that no longer verifies', async () => {
    get.mockResolvedValue(UNVERIFIED);
    const { result } = render();

    await waitFor(() => expect(result.current.response).toEqual(UNVERIFIED));
  });

  it('reports a refusal at once without asking again', async () => {
    get.mockRejectedValue(
      new ApiError('Failed to load the license: This needs a server admin.', HttpStatus.Forbidden)
    );
    const { result } = render();

    await waitFor(() =>
      expect(result.current.error).toBe('Failed to load the license: This needs a server admin.')
    );
    expect(result.current.response).toBeUndefined();
    expect(get).toHaveBeenCalledTimes(1);
  });

  for (const status of [
    HttpStatus.BadRequest,
    HttpStatus.Unauthorized,
    HttpStatus.NotFound,
    HttpStatus.TooManyRequests,
  ]) {
    it(`does not retry a ${status}`, async () => {
      get.mockRejectedValue(new ApiError(`Failed to load the license: ${status}`, status));
      const { result } = render();

      await waitFor(() => expect(result.current.error).not.toBeNull());
      expect(get).toHaveBeenCalledTimes(1);
    });
  }

  it('retries a server failure and shows the license once it loads', async () => {
    get
      .mockRejectedValueOnce(
        new ApiError('Failed to load the license: 500', HttpStatus.InternalServerError)
      )
      .mockResolvedValueOnce(COMMUNITY);
    const { result } = render();

    await waitFor(() => expect(result.current.response).toEqual(COMMUNITY));
    expect(result.current.error).toBeNull();
    expect(get).toHaveBeenCalledTimes(2);
  });

  it('gives up on a failure that keeps happening after the retry limit', async () => {
    get.mockRejectedValue(new ApiError('Failed to load the license: 502', HttpStatus.BadGateway));
    const { result } = render();

    await waitFor(() => expect(result.current.error).toBe('Failed to load the license: 502'));
    expect(get).toHaveBeenCalledTimes(RETRY_LIMIT + 1);
  });

  it('retries a network failure that never reached the server', async () => {
    get.mockRejectedValueOnce(new TypeError('Failed to fetch')).mockResolvedValueOnce(ACTIVE);
    const { result } = render();

    await waitFor(() => expect(result.current.response).toEqual(ACTIVE));
    expect(get).toHaveBeenCalledTimes(2);
  });

  it('loads again on refresh', async () => {
    get.mockResolvedValueOnce(COMMUNITY).mockResolvedValueOnce(ACTIVE);
    const { result } = render();
    await waitFor(() => expect(result.current.response).toEqual(COMMUNITY));

    await act(() => result.current.refresh());

    await waitFor(() => expect(result.current.response).toEqual(ACTIVE));
    expect(get).toHaveBeenCalledTimes(2);
  });
});
