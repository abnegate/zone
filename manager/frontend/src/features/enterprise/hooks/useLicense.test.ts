import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { ACTIVE, COMMUNITY } from '../fixtures';

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
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
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

  it('reports a failure to load', async () => {
    get.mockRejectedValue(new Error('Failed to load the license: This needs a server admin.'));
    const { result } = render();

    await waitFor(() =>
      expect(result.current.error).toBe('Failed to load the license: This needs a server admin.')
    );
    expect(result.current.response).toBeUndefined();
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
