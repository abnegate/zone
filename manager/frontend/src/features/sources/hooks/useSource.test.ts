/**
 * Tests for useSource hook
 */

import { renderHook, waitFor } from '@testing-library/react';
import { act } from 'react';
import { useSource } from './useSource';
import { sourcesApi } from '../../../api/sources';
import type { Source } from '../types';

jest.mock('../../../api/sources');

const mockSourcesApi = sourcesApi as jest.Mocked<typeof sourcesApi>;

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
    mockSourcesApi.getSource.mockResolvedValue(mockSource);

    const { result } = renderHook(() => useSource('1'));

    expect(result.current.loading).toBe(true);
    expect(result.current.source).toBeNull();

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.source).toEqual(mockSource);
    expect(result.current.error).toBeNull();
    expect(sourcesApi.getSource).toHaveBeenCalledWith('1');
  });

  it('should handle fetch error', async () => {
    const errorMessage = 'Failed to fetch source';
    mockSourcesApi.getSource.mockRejectedValue(new Error(errorMessage));

    const { result } = renderHook(() => useSource('1'));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.source).toBeNull();
    expect(result.current.error).toBe(errorMessage);
  });

  it('should update source', async () => {
    const updatedSource: Source = { ...mockSource, name: 'Updated Name' };
    mockSourcesApi.getSource.mockResolvedValue(mockSource);
    mockSourcesApi.updateSource.mockResolvedValue(updatedSource);

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
    mockSourcesApi.getSource.mockResolvedValue(mockSource);
    mockSourcesApi.verifySource.mockResolvedValue({
      success: true,
      message: 'Verified successfully',
    });
    mockSourcesApi.getSource.mockResolvedValueOnce(mockSource); // Initial load
    mockSourcesApi.getSource.mockResolvedValueOnce(verifiedSource); // After verify

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
    mockSourcesApi.getSource.mockResolvedValueOnce(mockSource); // Initial load
    mockSourcesApi.getSource.mockResolvedValueOnce(refreshedSource); // After refresh

    const { result } = renderHook(() => useSource('1'));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.source?.name).toBe('Test GitHub');

    await act(async () => {
      await result.current.refresh();
    });

    expect(result.current.source?.name).toBe('Refreshed Name');
    expect(sourcesApi.getSource).toHaveBeenCalledTimes(2);
  });

  it('should not fetch if id is null', () => {
    const { result } = renderHook(() => useSource(null));

    expect(result.current.loading).toBe(false);
    expect(result.current.source).toBeNull();
    expect(sourcesApi.getSource).not.toHaveBeenCalled();
  });
});
