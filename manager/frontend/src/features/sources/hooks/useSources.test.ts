/**
 * Tests for useSources hook
 */

import { renderHook, waitFor } from '@testing-library/react';
import { act } from 'react';
import { useSources } from './useSources';
import { sourcesApi } from '../../../api/sources';
import type { Source } from '../types';

jest.mock('../../../api/sources');

const mockSourcesApi = sourcesApi as jest.Mocked<typeof sourcesApi>;

describe('useSources', () => {
  const mockSources: Source[] = [
    {
      id: '1',
      name: 'Test GitHub',
      source_type: 'github',
      category: 'file',
      config: { owner: 'test', repo: 'repo' },
      description: null,
      url: 'https://github.com/test/repo',
      is_active: true,
      last_verified_at: null,
      last_error: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    },
    {
      id: '2',
      name: 'Test GitLab',
      source_type: 'gitlab',
      category: 'file',
      config: { project_id: '123' },
      description: 'Test description',
      url: 'https://gitlab.com/test/project',
      is_active: false,
      last_verified_at: '2024-01-01T00:00:00Z',
      last_error: 'Connection failed',
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    },
  ];

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('should fetch sources on mount', async () => {
    mockSourcesApi.getSources.mockResolvedValue(mockSources);

    const { result } = renderHook(() => useSources());

    expect(result.current.loading).toBe(true);
    expect(result.current.sources).toEqual([]);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.sources).toEqual(mockSources);
    expect(result.current.error).toBeNull();
    expect(sourcesApi.getSources).toHaveBeenCalledTimes(1);
  });

  it('should handle fetch error', async () => {
    const errorMessage = 'Failed to fetch sources';
    mockSourcesApi.getSources.mockRejectedValue(new Error(errorMessage));

    const { result } = renderHook(() => useSources());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.sources).toEqual([]);
    expect(result.current.error).toBe(errorMessage);
  });

  it('should filter sources by type', async () => {
    mockSourcesApi.getSources.mockResolvedValue(mockSources);

    const { result } = renderHook(() => useSources({ type: 'github' }));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(sourcesApi.getSources).toHaveBeenCalledWith('github', false);
  });

  it('should filter active sources only', async () => {
    mockSourcesApi.getSources.mockResolvedValue([mockSources[0]]);

    const { result } = renderHook(() => useSources({ activeOnly: true }));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(sourcesApi.getSources).toHaveBeenCalledWith(undefined, true);
  });

  it('should create source', async () => {
    const newSource: Source = { ...mockSources[0], id: '3', name: 'New Source' };
    mockSourcesApi.getSources.mockResolvedValue(mockSources);
    mockSourcesApi.createSource.mockResolvedValue(newSource);

    const { result } = renderHook(() => useSources());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await result.current.createSource({
        name: 'New Source',
        source_type: 'github',
        config: { owner: 'test', repo: 'new' },
      });
    });

    expect(result.current.sources).toContainEqual(newSource);
  });

  it('should update source', async () => {
    const updatedSource: Source = { ...mockSources[0], name: 'Updated Name' };
    mockSourcesApi.getSources.mockResolvedValue(mockSources);
    mockSourcesApi.updateSource.mockResolvedValue(updatedSource);

    const { result } = renderHook(() => useSources());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await result.current.updateSource('1', { name: 'Updated Name' });
    });

    expect(result.current.sources.find((s) => s.id === '1')?.name).toBe('Updated Name');
  });

  it('should delete source', async () => {
    mockSourcesApi.getSources.mockResolvedValue(mockSources);
    mockSourcesApi.deleteSource.mockResolvedValue();

    const { result } = renderHook(() => useSources());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.sources).toHaveLength(2);

    await act(async () => {
      await result.current.deleteSource('1');
    });

    expect(result.current.sources).toHaveLength(1);
    expect(result.current.sources.find((s) => s.id === '1')).toBeUndefined();
  });

  it('should verify source', async () => {
    const verifiedSource: Source = {
      ...mockSources[0],
      last_verified_at: '2024-01-02T00:00:00Z',
    };
    mockSourcesApi.getSources.mockResolvedValue(mockSources);
    mockSourcesApi.verifySource.mockResolvedValue({
      success: true,
      message: 'Verified successfully',
    });
    mockSourcesApi.getSource.mockResolvedValue(verifiedSource);

    const { result } = renderHook(() => useSources());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    let verifyResult: { success: boolean; message: string } | undefined;
    await act(async () => {
      verifyResult = await result.current.verifySource('1');
    });

    expect(verifyResult).toEqual({ success: true, message: 'Verified successfully' });
    expect(result.current.sources.find((s) => s.id === '1')?.last_verified_at).toBe(
      '2024-01-02T00:00:00Z'
    );
  });

  it('should refresh sources', async () => {
    mockSourcesApi.getSources.mockResolvedValue(mockSources);

    const { result } = renderHook(() => useSources());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(sourcesApi.getSources).toHaveBeenCalledTimes(1);

    await act(async () => {
      await result.current.refresh();
    });

    expect(sourcesApi.getSources).toHaveBeenCalledTimes(2);
  });
});
