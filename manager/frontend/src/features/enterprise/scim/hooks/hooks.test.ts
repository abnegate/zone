import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { ApiError } from '../../../../api/ApiError';
import {
  ACTIVE,
  ENGINEERING,
  ENTRA,
  ISSUED,
  OKTA,
  ORGANIZATION_ID,
  ROTATED,
  SECRET,
} from '../fixtures';
import { scimQueryKeys } from '../queryKeys';
import { ScimRefusal } from '../ScimRefusal';
import { MappedRole, ScimCode } from '../schemas';
import type { ScimSettings } from '../types';

const settings = mock();
const createToken = mock();
const rotateToken = mock();
const revokeToken = mock();
const mapGroup = mock();
const unmapGroup = mock();

const CALLS = [settings, createToken, rotateToken, revokeToken, mapGroup, unmapGroup];

mock.module('../api', () => ({
  scimApi: { settings, createToken, rotateToken, revokeToken, mapGroup, unmapGroup },
}));

let hooks: typeof import('./index');

beforeAll(async () => {
  hooks = await import('./index');
});

afterAll(() => {
  mock.restore();
});

function client(): QueryClient {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity }, mutations: { retry: false } },
  });
}

function wrapper(queryClient: QueryClient) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
}

function seeded(): QueryClient {
  const queryClient = client();
  queryClient.setQueryData(scimQueryKeys.organization(ORGANIZATION_ID), ACTIVE);
  return queryClient;
}

function cached(queryClient: QueryClient): ScimSettings | undefined {
  return queryClient.getQueryData(scimQueryKeys.organization(ORGANIZATION_ID));
}

function holds(queryClient: QueryClient, secret: string): boolean {
  const queries = queryClient.getQueryCache().getAll();
  const mutations = queryClient.getMutationCache().getAll();
  return JSON.stringify([
    queries.map((query) => query.state.data),
    mutations.map((mutation) => [mutation.state.data, mutation.state.variables]),
  ]).includes(secret);
}

beforeEach(() => {
  for (const call of CALLS) {
    call.mockReset();
  }
});

describe('useScim', () => {
  it('loads the SCIM settings of the organization', async () => {
    settings.mockResolvedValue(ACTIVE);
    const { result } = renderHook(() => hooks.useScim(ORGANIZATION_ID), {
      wrapper: wrapper(client()),
    });

    await waitFor(() => expect(result.current.settings).toEqual(ACTIVE));
    expect(settings).toHaveBeenCalledWith(ORGANIZATION_ID);
    expect(result.current.error).toBeNull();
  });

  it('waits for an organization before loading', () => {
    const { result } = renderHook(() => hooks.useScim(null), { wrapper: wrapper(client()) });

    expect(result.current.settings).toBeNull();
    expect(settings).not.toHaveBeenCalled();
  });

  it('reports a refused load without retrying it', async () => {
    settings.mockRejectedValue(new ApiError('Failed to load SCIM provisioning: Forbidden', 403));
    const { result } = renderHook(() => hooks.useScim(ORGANIZATION_ID), {
      wrapper: wrapper(client()),
    });

    await waitFor(() =>
      expect(result.current.error).toBe('Failed to load SCIM provisioning: Forbidden')
    );
    expect(settings).toHaveBeenCalledTimes(1);
  });
});

describe('useCreateScimToken', () => {
  it('hands the secret to the caller once and caches only the token', async () => {
    createToken.mockResolvedValue(ISSUED);
    settings.mockImplementation(() => new Promise(() => {}));
    const queryClient = seeded();
    const issued = mock();
    const { result } = renderHook(() => hooks.useCreateScimToken(ORGANIZATION_ID), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.create('Google Workspace', issued));

    await waitFor(() => expect(issued).toHaveBeenCalledWith(ISSUED));
    expect(issued).toHaveBeenCalledTimes(1);
    expect(createToken).toHaveBeenCalledWith(ORGANIZATION_ID, 'Google Workspace');
    expect(cached(queryClient)?.tokens).toEqual([OKTA, ENTRA, ISSUED.token]);
    await waitFor(() => expect(holds(queryClient, SECRET)).toBe(false));
  });

  it('exposes a refused name and keeps the cached tokens', async () => {
    createToken.mockRejectedValue(
      new ScimRefusal({ error: 'Name', code: ScimCode.TokenNameInvalid }, 400)
    );
    settings.mockResolvedValue(ACTIVE);
    const queryClient = seeded();
    const { result } = renderHook(() => hooks.useCreateScimToken(ORGANIZATION_ID), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.create(' ', mock()));

    await waitFor(() => expect(result.current.refusal?.code).toBe(ScimCode.TokenNameInvalid));
    expect(cached(queryClient)?.tokens).toEqual(ACTIVE.tokens);
  });
});

describe('useRotateScimToken', () => {
  it('hands the new secret to the caller and caches only the rotated token', async () => {
    rotateToken.mockResolvedValue(ROTATED);
    settings.mockImplementation(() => new Promise(() => {}));
    const queryClient = seeded();
    const issued = mock();
    const { result } = renderHook(() => hooks.useRotateScimToken(ORGANIZATION_ID), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.rotate(OKTA.id, issued));

    await waitFor(() => expect(issued).toHaveBeenCalledWith(ROTATED));
    expect(rotateToken).toHaveBeenCalledWith(ORGANIZATION_ID, OKTA.id);
    expect(cached(queryClient)?.tokens).toEqual([ROTATED.token, ENTRA]);
    await waitFor(() => expect(holds(queryClient, SECRET)).toBe(false));
  });

  it('exposes a lapsed license', async () => {
    rotateToken.mockRejectedValue(
      new ScimRefusal({ error: 'Enterprise', code: ScimCode.LicenseRequired }, 403)
    );
    settings.mockResolvedValue(ACTIVE);
    const { result } = renderHook(() => hooks.useRotateScimToken(ORGANIZATION_ID), {
      wrapper: wrapper(seeded()),
    });

    act(() => result.current.rotate(OKTA.id, mock()));

    await waitFor(() => expect(result.current.refusal?.code).toBe(ScimCode.LicenseRequired));
  });
});

describe('useRevokeScimToken', () => {
  it('drops the revoked token from the cache', async () => {
    revokeToken.mockResolvedValue(undefined);
    settings.mockImplementation(() => new Promise(() => {}));
    const queryClient = seeded();
    const revoked = mock();
    const { result } = renderHook(() => hooks.useRevokeScimToken(ORGANIZATION_ID), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.revoke(OKTA.id, revoked));

    await waitFor(() => expect(revoked).toHaveBeenCalled());
    expect(revokeToken).toHaveBeenCalledWith(ORGANIZATION_ID, OKTA.id);
    expect(cached(queryClient)?.tokens).toEqual([ENTRA]);
  });
});

describe('useMapScimGroup and useUnmapScimGroup', () => {
  it('replaces the mapped group in the cache', async () => {
    const saved = { ...ENGINEERING, role: MappedRole.Admin };
    mapGroup.mockResolvedValue(saved);
    settings.mockImplementation(() => new Promise(() => {}));
    const queryClient = seeded();
    const { result } = renderHook(() => hooks.useMapScimGroup(ORGANIZATION_ID), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.map(ENGINEERING.id, { role: MappedRole.Admin }));

    await waitFor(() => expect(cached(queryClient)?.groups[0]).toEqual(saved));
    expect(mapGroup).toHaveBeenCalledWith(ORGANIZATION_ID, ENGINEERING.id, {
      role: MappedRole.Admin,
    });
  });

  it('replaces the unmapped group in the cache', async () => {
    const saved = { ...ENGINEERING, role: null };
    unmapGroup.mockResolvedValue(saved);
    settings.mockImplementation(() => new Promise(() => {}));
    const queryClient = seeded();
    const { result } = renderHook(() => hooks.useUnmapScimGroup(ORGANIZATION_ID), {
      wrapper: wrapper(queryClient),
    });

    act(() => result.current.unmap(ENGINEERING.id));

    await waitFor(() => expect(cached(queryClient)?.groups[0]).toEqual(saved));
    expect(unmapGroup).toHaveBeenCalledWith(ORGANIZATION_ID, ENGINEERING.id);
  });

  it('exposes a mapping past the caller', async () => {
    mapGroup.mockRejectedValue(new ScimRefusal({ error: 'Exceeds', code: ScimCode.Exceeds }, 403));
    settings.mockResolvedValue(ACTIVE);
    const { result } = renderHook(() => hooks.useMapScimGroup(ORGANIZATION_ID), {
      wrapper: wrapper(seeded()),
    });

    act(() => result.current.map(ENGINEERING.id, { custom_role_id: 'role-1' }));

    await waitFor(() => expect(result.current.refusal?.code).toBe(ScimCode.Exceeds));
  });
});
