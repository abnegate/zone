/**
 * Tests for useSource hook
 */

import { renderHook, waitFor } from '@testing-library/react';
import { act } from 'react';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
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

let useSource: typeof import('./useSource').useSource;

beforeAll(async () => {
  ({ useSource } = await import('./useSource'));
});

afterAll(() => {
  mock.restore();
});

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
    jest.clearAllMocks();
  });

  it('should fetch source on mount', async () => {
    mockGetSource.mockResolvedValue(mockSource);

    const { result } = renderHook(() => useSource('1'));

    expect(result.current.loading).toBe(true);
    expect(result.current.source).toBeNull();

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.source).toEqual(mockSource);
    expect(result.current.error).toBeNull();
    expect(mockGetSource).toHaveBeenCalledWith('1');
  });

  it('should handle fetch error', async () => {
    const errorMessage = 'Failed to fetch source';
    mockGetSource.mockRejectedValue(new Error(errorMessage));

    const { result } = renderHook(() => useSource('1'));

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

    const { result } = renderHook(() => useSource('1'));

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
      success: true,
      message: 'Verified successfully',
    });
    mockGetSource.mockResolvedValueOnce(mockSource); // Initial load
    mockGetSource.mockResolvedValueOnce(verifiedSource); // After verify

    const { result } = renderHook(() => useSource('1'));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    let verifyResult: { success: boolean; message: string } | undefined;
    await act(async () => {
      verifyResult = await result.current.verifySource();
    });

    expect(verifyResult).toEqual({ success: true, message: 'Verified successfully' });
    expect(result.current.source?.last_verified_at).toBe('2024-01-02T00:00:00Z');
  });

  it('should refresh source', async () => {
    const refreshedSource: Source = { ...mockSource, name: 'Refreshed Name' };
    mockGetSource.mockResolvedValueOnce(mockSource); // Initial load
    mockGetSource.mockResolvedValueOnce(refreshedSource); // After refresh

    const { result } = renderHook(() => useSource('1'));

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
    const { result } = renderHook(() => useSource(null));

    expect(result.current.loading).toBe(false);
    expect(result.current.source).toBeNull();
    expect(mockGetSource).not.toHaveBeenCalled();
  });
});
