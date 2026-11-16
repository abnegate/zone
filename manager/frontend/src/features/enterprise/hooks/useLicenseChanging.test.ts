import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { ACTIVE, COMMUNITY } from '../fixtures';

const get = mock();
const set = mock();
const remove = mock();

mock.module('../api', () => ({ licenseApi: { get, set, remove } }));

let hooks: typeof import('.');

beforeAll(async () => {
  hooks = await import('.');
});

afterAll(() => {
  mock.restore();
});

function setup() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
  return renderHook(
    () => ({
      changing: hooks.useLicenseChanging(),
      setter: hooks.useSetLicense(),
      remover: hooks.useRemoveLicense(),
    }),
    { wrapper }
  );
}

function deferred(): { promise: Promise<unknown>; settle: (value: unknown) => void } {
  let settle: (value: unknown) => void = () => {};
  const promise = new Promise((resolve) => (settle = resolve));
  return { promise, settle };
}

describe('useLicenseChanging', () => {
  beforeEach(() => {
    get.mockReset();
    set.mockReset();
    remove.mockReset();
  });

  it('is false while nothing changes the license', () => {
    const { result } = setup();

    expect(result.current.changing).toBe(false);
  });

  it('is true while a key is saved and false once it settles', async () => {
    const save = deferred();
    set.mockImplementation(() => save.promise);
    const { result } = setup();

    act(() => result.current.setter.set('payload.signature'));

    await waitFor(() => expect(result.current.changing).toBe(true));
    await act(async () => save.settle(ACTIVE));
    await waitFor(() => expect(result.current.changing).toBe(false));
  });

  it('is true while the license is removed and false once it settles', async () => {
    const removal = deferred();
    remove.mockImplementation(() => removal.promise);
    const { result } = setup();

    act(() => result.current.remover.remove());

    await waitFor(() => expect(result.current.changing).toBe(true));
    await act(async () => removal.settle(COMMUNITY));
    await waitFor(() => expect(result.current.changing).toBe(false));
  });
});
