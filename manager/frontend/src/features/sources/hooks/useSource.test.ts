/**
 * Tests for useSource hook
 */

import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { act, createElement } from 'react';
import type { Source } from '../types';

const mockGetSource = mock();
const mockUpdateSource = mock();
const mockVerifySource = mock();

mock.module('../../../api/sources', () => ({
  sourcesApi: {
    getSource: mockGetSource,
    updateSource: mockUpdateSource,
    verifySource: mockVerifySource,
  },
}));

// Mock useWorkspace to provide a test workspace
mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentWorkspace: { id: 'test-workspace-id', name: 'Test Workspace' },
    currentOrganization: { id: 'test-org-id', name: 'Test Org' },
    workspaces: [],
    organizations: [],
    loading: false,
    error: null,
    setCurrentWorkspace: mock(),
    setCurrentOrganization: mock(),
    refreshWorkspaces: mock(),
    refreshOrganizations: mock(),
  }),
}));

let useSource: typeof import('./useSource').useSource;

beforeAll(async () => {
  ({ useSource } = await import('./useSource'));
});

afterAll(() => {
  mock.restore();
});

const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false, gcTime: 0 },
    },
  });
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
};

describe('useSource', () => {
  const mockSource: Source = {
    id: '1',
    name: 'Test GitHub',
    source_type: 'github',
    category: 'file',
    config: { owner: 'test', repo: 'repo' },
    description: 'Test description',
    url: 'https://github.com/test/repo',
    is_active: true,
    last_verified_at: null,
    last_error: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
  };

  beforeEach(() => {
    mockGetSource.mockReset();
    mockUpdateSource.mockReset();
    mockVerifySource.mockReset();
  });

  it('should fetch source on mount', async () => {
    mockGetSource.mockResolvedValue(mockSource);

    const { result } = renderHook(() => useSource('1'), { wrapper: createWrapper() });

    expect(result.current.loading).toBe(true);
    expect(result.current.source).toBeNull();

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.source).toEqual(mockSource);
    expect(result.current.error).toBeNull();
    expect(mockGetSource).toHaveBeenCalledWith('test-workspace-id', '1');
  });

  it('should handle fetch error', async () => {
    const errorMessage = 'Failed to fetch source';
    mockGetSource.mockRejectedValue(new Error(errorMessage));

    const { result } = renderHook(() => useSource('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.source).toBeNull();
    expect(result.current.error).toBe(errorMessage);
  });

  it('should update source', async () => {
    const updatedSource: Source = { ...mockSource, name: 'Updated Name' };
    mockGetSource.mockResolvedValue(mockSource);
    mockUpdateSource.mockResolvedValue(updatedSource);

    const { result } = renderHook(() => useSource('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await result.current.updateSource({ name: 'Updated Name' });
    });

    expect(result.current.source?.name).toBe('Updated Name');
  });

  it('should verify source', async () => {
    const verifiedSource: Source = {
      ...mockSource,
      last_verified_at: '2024-01-02T00:00:00Z',
    };
    mockGetSource.mockResolvedValue(mockSource);
    mockVerifySource.mockResolvedValue({
      verified: true,
      message: 'Verified successfully',
    });
    mockGetSource.mockResolvedValueOnce(mockSource); // Initial load
    mockGetSource.mockResolvedValueOnce(verifiedSource); // After verify

    const { result } = renderHook(() => useSource('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    let verifyResult: { verified: boolean; message: string } | undefined;
    await act(async () => {
      verifyResult = await result.current.verifySource();
    });

    expect(verifyResult).toEqual({ verified: true, message: 'Verified successfully' });
    expect(result.current.source?.last_verified_at).toBe('2024-01-02T00:00:00Z');
  });

  it('refreshes the source error after unsuccessful verification', async () => {
    const message = 'Authentication failed - check your credentials';
    const failedSource: Source = { ...mockSource, last_error: message };
    mockGetSource.mockResolvedValueOnce(mockSource);
    mockVerifySource.mockResolvedValue({ verified: false, message });
    mockGetSource.mockResolvedValueOnce(failedSource);

    const { result } = renderHook(() => useSource('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      expect(await result.current.verifySource()).toEqual({ verified: false, message });
    });

    expect(result.current.source?.last_error).toBe(message);
    expect(mockGetSource).toHaveBeenCalledWith('test-workspace-id', '1');
  });

  it('should refresh source', async () => {
    const refreshedSource: Source = { ...mockSource, name: 'Refreshed Name' };
    mockGetSource.mockResolvedValueOnce(mockSource); // Initial load
    mockGetSource.mockResolvedValueOnce(refreshedSource); // After refresh

    const { result } = renderHook(() => useSource('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.source?.name).toBe('Test GitHub');

    await act(async () => {
      await result.current.refresh();
    });

    expect(result.current.source?.name).toBe('Refreshed Name');
    expect(mockGetSource).toHaveBeenCalledTimes(2);
  });

  it('should not fetch if id is null', () => {
    const { result } = renderHook(() => useSource(null), { wrapper: createWrapper() });

    expect(result.current.loading).toBe(false);
    expect(result.current.source).toBeNull();
    expect(mockGetSource).not.toHaveBeenCalled();
  });
});
