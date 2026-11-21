import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { ApiError } from '../../../../api/ApiError';
import { AuditRefusal } from '../AuditRefusal';
import {
  DELIVERED_TEST,
  ENTERPRISE_DESTINATIONS,
  ENTERPRISE_RETENTION,
  FAILED_TEST,
  FAILING_WEBHOOK,
  HEALTHY_S3,
  ORGANIZATION_SCOPE,
  SERVER_DESTINATIONS,
} from '../fixtures';
import { auditQueryKeys } from '../queryKeys';
import { AuditCode, AuditKind } from '../schemas';
import { SERVER_SCOPE } from '../scope';
import type { DestinationsResponse } from '../types';
import { AUDIT_RETRY_LIMIT, retryLoad } from './retry';

const destinations = mock();
const createDestination = mock();
const updateDestination = mock();
const deleteDestination = mock();
const testDestination = mock();

mock.module('../api', () => ({
  auditApi: {
    destinations,
    destination: mock(),
    createDestination,
    updateDestination,
    deleteDestination,
    testDestination,
    retention: mock(),
    setRetention: mock(),
    download: mock(),
  },
}));

let useAuditDestinations: typeof import('./useAuditDestinations').useAuditDestinations;
let useCreateAuditDestination: typeof import('./useCreateAuditDestination').useCreateAuditDestination;
let useUpdateAuditDestination: typeof import('./useUpdateAuditDestination').useUpdateAuditDestination;
let useDeleteAuditDestination: typeof import('./useDeleteAuditDestination').useDeleteAuditDestination;
let useTestAuditDestination: typeof import('./useTestAuditDestination').useTestAuditDestination;

beforeAll(async () => {
  ({ useAuditDestinations } = await import('./useAuditDestinations'));
  ({ useCreateAuditDestination } = await import('./useCreateAuditDestination'));
  ({ useUpdateAuditDestination } = await import('./useUpdateAuditDestination'));
  ({ useDeleteAuditDestination } = await import('./useDeleteAuditDestination'));
  ({ useTestAuditDestination } = await import('./useTestAuditDestination'));
});

afterAll(() => {
  mock.restore();
});

const LIST_KEY = auditQueryKeys.destinations(ORGANIZATION_SCOPE);

const RETENTION_KEY = auditQueryKeys.retention(ORGANIZATION_SCOPE);

const CREATION = {
  name: 'Audit archive',
  settings: { kind: AuditKind.S3, bucket: 'acme-audit', region: 'eu-west-1' },
  secrets: { access_key_id: 'AKIA', secret_access_key: 'secret' },
} as const;

function client(): QueryClient {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity }, mutations: { retry: false } },
  });
}

function seeded(): QueryClient {
  const queryClient = client();
  queryClient.setQueryData(LIST_KEY, {
    ...ENTERPRISE_DESTINATIONS,
    destinations: [FAILING_WEBHOOK],
  });
  queryClient.setQueryData(RETENTION_KEY, ENTERPRISE_RETENTION);
  return queryClient;
}

function wrapper(queryClient: QueryClient) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
}

function listed(queryClient: QueryClient): DestinationsResponse['destinations'] | undefined {
  return queryClient.getQueryData<DestinationsResponse>(LIST_KEY)?.destinations;
}

function invalidated(queryClient: QueryClient): boolean | undefined {
  return queryClient.getQueryState(RETENTION_KEY)?.isInvalidated;
}

describe('useAuditDestinations', () => {
  beforeEach(() => {
    destinations.mockReset();
  });

  it('loads the destinations of the scope', async () => {
    destinations.mockResolvedValue(ENTERPRISE_DESTINATIONS);
    const { result, unmount } = renderHook(() => useAuditDestinations(ORGANIZATION_SCOPE), {
      wrapper: wrapper(client()),
    });

    await waitFor(() => expect(result.current.destinations).toEqual(ENTERPRISE_DESTINATIONS));
    expect(destinations).toHaveBeenCalledWith(ORGANIZATION_SCOPE);
    expect(result.current.error).toBeNull();
    unmount();
  });

  it('keeps the organization and server lists apart', async () => {
    destinations.mockResolvedValue(SERVER_DESTINATIONS);
    const queryClient = client();
    const { result, unmount } = renderHook(() => useAuditDestinations(SERVER_SCOPE), {
      wrapper: wrapper(queryClient),
    });

    await waitFor(() => expect(result.current.destinations).toEqual(SERVER_DESTINATIONS));
    expect(queryClient.getQueryData(LIST_KEY)).toBeUndefined();
    unmount();
  });

  it('waits for a scope before loading', () => {
    const { result } = renderHook(() => useAuditDestinations(null), {
      wrapper: wrapper(client()),
    });

    expect(result.current.destinations).toBeUndefined();
    expect(result.current.loading).toBe(false);
    expect(destinations).not.toHaveBeenCalled();
  });

  it('reports a refused load without retrying it', async () => {
    destinations.mockRejectedValue(
      new ApiError('Failed to load the audit destinations: Forbidden', 403)
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retryDelay: 0 } } });
    const { result } = renderHook(() => useAuditDestinations(ORGANIZATION_SCOPE), {
      wrapper: wrapper(queryClient),
    });

    await waitFor(() =>
      expect(result.current.error).toBe('Failed to load the audit destinations: Forbidden')
    );
    expect(destinations).toHaveBeenCalledTimes(1);
  });
});

describe('retryLoad', () => {
  it('retries a server failure a few times and never a refusal', () => {
    expect(retryLoad(0, new ApiError('Bad gateway', 502))).toBe(true);
    expect(retryLoad(AUDIT_RETRY_LIMIT, new ApiError('Bad gateway', 502))).toBe(false);
    expect(retryLoad(0, new Error('offline'))).toBe(true);
    expect(retryLoad(0, new ApiError('Not found', 404))).toBe(false);
    expect(
      retryLoad(0, new AuditRefusal({ error: 'x', code: AuditCode.LicenseRequired }, 403))
    ).toBe(false);
  });
});

describe('useCreateAuditDestination', () => {
  beforeEach(() => {
    createDestination.mockReset();
  });

  it('adds the destination to the list and refreshes the scope', async () => {
    createDestination.mockResolvedValue(HEALTHY_S3);
    const queryClient = seeded();
    const created = mock();
    const { result } = renderHook(() => useCreateAuditDestination(ORGANIZATION_SCOPE), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.create(CREATION, created));

    await waitFor(() => expect(created).toHaveBeenCalledWith(HEALTHY_S3));
    expect(createDestination).toHaveBeenCalledWith(ORGANIZATION_SCOPE, CREATION);
    expect(listed(queryClient)).toEqual([FAILING_WEBHOOK, HEALTHY_S3]);
    expect(invalidated(queryClient)).toBe(true);
  });

  it('exposes a refusal and leaves the list alone', async () => {
    createDestination.mockRejectedValue(
      new AuditRefusal({ error: 'Duplicate', code: AuditCode.DestinationDuplicate }, 409)
    );
    const queryClient = seeded();
    const { result } = renderHook(() => useCreateAuditDestination(ORGANIZATION_SCOPE), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.create(CREATION));

    await waitFor(() => expect(result.current.refusal?.code).toBe(AuditCode.DestinationDuplicate));
    expect(result.current.error).toBe('Duplicate');
    expect(listed(queryClient)).toEqual([FAILING_WEBHOOK]);

    act(() => result.current.reset());
    await waitFor(() => expect(result.current.refusal).toBeNull());
  });
});

describe('useUpdateAuditDestination', () => {
  beforeEach(() => {
    updateDestination.mockReset();
  });

  it('replaces the destination in the list and refreshes the scope', async () => {
    const disabled = { ...FAILING_WEBHOOK, enabled: false };
    updateDestination.mockResolvedValue(disabled);
    const queryClient = seeded();
    const updated = mock();
    const { result } = renderHook(() => useUpdateAuditDestination(ORGANIZATION_SCOPE), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.update(FAILING_WEBHOOK.id, { enabled: false }, updated));

    await waitFor(() => expect(updated).toHaveBeenCalledWith(disabled));
    expect(updateDestination).toHaveBeenCalledWith(ORGANIZATION_SCOPE, FAILING_WEBHOOK.id, {
      enabled: false,
    });
    expect(listed(queryClient)).toEqual([disabled]);
    expect(invalidated(queryClient)).toBe(true);
  });

  it('exposes a license refusal', async () => {
    updateDestination.mockRejectedValue(
      new AuditRefusal({ error: 'License required', code: AuditCode.LicenseRequired }, 403)
    );
    const { result } = renderHook(() => useUpdateAuditDestination(ORGANIZATION_SCOPE), {
      wrapper: wrapper(seeded()),
    });

    act(() => result.current.update(FAILING_WEBHOOK.id, { name: 'SIEM' }));

    await waitFor(() => expect(result.current.refusal?.status).toBe(403));
  });
});

describe('useDeleteAuditDestination', () => {
  beforeEach(() => {
    deleteDestination.mockReset();
  });

  it('drops the destination and refreshes the scope', async () => {
    deleteDestination.mockResolvedValue(undefined);
    const queryClient = seeded();
    const deleted = mock();
    const { result } = renderHook(() => useDeleteAuditDestination(ORGANIZATION_SCOPE), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.remove(FAILING_WEBHOOK.id, deleted));

    await waitFor(() => expect(deleted).toHaveBeenCalledTimes(1));
    expect(deleteDestination).toHaveBeenCalledWith(ORGANIZATION_SCOPE, FAILING_WEBHOOK.id);
    expect(listed(queryClient)).toEqual([]);
    expect(invalidated(queryClient)).toBe(true);
  });

  it('keeps the destination when the delete is refused', async () => {
    deleteDestination.mockRejectedValue(
      new AuditRefusal({ error: 'Gone', code: AuditCode.DestinationNotFound }, 404)
    );
    const queryClient = seeded();
    const deleted = mock();
    const { result } = renderHook(() => useDeleteAuditDestination(ORGANIZATION_SCOPE), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.remove(FAILING_WEBHOOK.id, deleted));

    await waitFor(() => expect(result.current.refusal?.code).toBe(AuditCode.DestinationNotFound));
    expect(deleted).not.toHaveBeenCalled();
    expect(listed(queryClient)).toEqual([FAILING_WEBHOOK]);
  });
});

describe('useTestAuditDestination', () => {
  beforeEach(() => {
    testDestination.mockReset();
  });

  it('reports which destination it tested and the result', async () => {
    testDestination.mockResolvedValue(FAILED_TEST);
    const tested = mock();
    const { result } = renderHook(() => useTestAuditDestination(ORGANIZATION_SCOPE), {
      wrapper: wrapper(client()),
    });

    act(() => result.current.test(FAILING_WEBHOOK.id, tested));

    await waitFor(() => expect(tested).toHaveBeenCalledWith(FAILED_TEST));
    expect(testDestination).toHaveBeenCalledWith(ORGANIZATION_SCOPE, FAILING_WEBHOOK.id);
    expect(result.current.result).toEqual(FAILED_TEST);
    expect(result.current.testing).toBe(FAILING_WEBHOOK.id);
  });

  it('forgets the last result on reset', async () => {
    testDestination.mockResolvedValue(DELIVERED_TEST);
    const { result } = renderHook(() => useTestAuditDestination(SERVER_SCOPE), {
      wrapper: wrapper(client()),
    });

    act(() => result.current.test(HEALTHY_S3.id));
    await waitFor(() => expect(result.current.result).toEqual(DELIVERED_TEST));

    act(() => result.current.reset());
    await waitFor(() => expect(result.current.result).toBeUndefined());
    expect(result.current.testing).toBeNull();
  });
});

describe('auditQueryKeys', () => {
  it('keys each scope apart under audit', () => {
    expect(auditQueryKeys.destinations(ORGANIZATION_SCOPE)).toEqual([
      'audit',
      'organization',
      ORGANIZATION_SCOPE.kind === 'organization' ? ORGANIZATION_SCOPE.organizationId : '',
      'destinations',
    ]);
    expect(auditQueryKeys.retention(SERVER_SCOPE)).toEqual(['audit', 'server', 'retention']);
    expect(auditQueryKeys.scope(SERVER_SCOPE)).toEqual(['audit', 'server']);
  });
});
