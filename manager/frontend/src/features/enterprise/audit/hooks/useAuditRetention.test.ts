import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { AuditRefusal } from '../AuditRefusal';
import {
  ENTERPRISE_RETENTION,
  FOREVER_RETENTION,
  ORGANIZATION_SCOPE,
  WAITING_RETENTION,
} from '../fixtures';
import { auditQueryKeys } from '../queryKeys';
import { AuditCode } from '../schemas';

const retention = mock();
const setRetention = mock();

mock.module('../api', () => ({
  auditApi: {
    destinations: mock(),
    destination: mock(),
    createDestination: mock(),
    updateDestination: mock(),
    deleteDestination: mock(),
    testDestination: mock(),
    retention,
    setRetention,
    download: mock(),
  },
}));

let useAuditRetention: typeof import('./useAuditRetention').useAuditRetention;
let useSetAuditRetention: typeof import('./useSetAuditRetention').useSetAuditRetention;

beforeAll(async () => {
  ({ useAuditRetention } = await import('./useAuditRetention'));
  ({ useSetAuditRetention } = await import('./useSetAuditRetention'));
});

afterAll(() => {
  mock.restore();
});

const KEY = auditQueryKeys.retention(ORGANIZATION_SCOPE);

function client(): QueryClient {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity }, mutations: { retry: false } },
  });
}

function wrapper(queryClient: QueryClient) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
}

describe('useAuditRetention', () => {
  beforeEach(() => {
    retention.mockReset();
  });

  it('loads the retention of the scope, with what it waits on', async () => {
    retention.mockResolvedValue(WAITING_RETENTION);
    const { result } = renderHook(() => useAuditRetention(ORGANIZATION_SCOPE), {
      wrapper: wrapper(client()),
    });

    await waitFor(() => expect(result.current.retention).toEqual(WAITING_RETENTION));
    expect(retention).toHaveBeenCalledWith(ORGANIZATION_SCOPE);
  });

  it('waits for a scope before loading', () => {
    renderHook(() => useAuditRetention(null), { wrapper: wrapper(client()) });

    expect(retention).not.toHaveBeenCalled();
  });
});

describe('useSetAuditRetention', () => {
  beforeEach(() => {
    setRetention.mockReset();
  });

  it('saves the days and caches the result', async () => {
    setRetention.mockResolvedValue(ENTERPRISE_RETENTION);
    const queryClient = client();
    queryClient.setQueryData(KEY, FOREVER_RETENTION);
    const saved = mock();
    const { result } = renderHook(() => useSetAuditRetention(ORGANIZATION_SCOPE), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.set(90, saved));

    await waitFor(() => expect(saved).toHaveBeenCalledWith(ENTERPRISE_RETENTION));
    expect(setRetention).toHaveBeenCalledWith(ORGANIZATION_SCOPE, { days: 90 });
    expect(queryClient.getQueryData(KEY)).toEqual(ENTERPRISE_RETENTION);
  });

  it('sends null to keep events forever', async () => {
    setRetention.mockResolvedValue(FOREVER_RETENTION);
    const { result } = renderHook(() => useSetAuditRetention(ORGANIZATION_SCOPE), {
      wrapper: wrapper(client()),
    });

    act(() => result.current.set(null));

    await waitFor(() =>
      expect(setRetention).toHaveBeenCalledWith(ORGANIZATION_SCOPE, { days: null })
    );
  });

  it('exposes a lapsed refusal and keeps the cached retention', async () => {
    setRetention.mockRejectedValue(
      new AuditRefusal({ error: 'License required', code: AuditCode.LicenseRequired }, 403)
    );
    const queryClient = client();
    queryClient.setQueryData(KEY, ENTERPRISE_RETENTION);
    const { result } = renderHook(() => useSetAuditRetention(ORGANIZATION_SCOPE), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.set(null));

    await waitFor(() => expect(result.current.refusal?.code).toBe(AuditCode.LicenseRequired));
    expect(queryClient.getQueryData(KEY)).toEqual(ENTERPRISE_RETENTION);
  });
});
