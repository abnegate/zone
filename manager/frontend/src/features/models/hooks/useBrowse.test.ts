import { act, renderHook, waitFor } from '@testing-library/react';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';

const mockBrowseModels = mock();

// State container for auth mock that can be updated per test
let authState = {
  isAuthenticated: true,
};

mock.module('../../../api/models', () => ({
  modelsApi: {
    browseModels: mockBrowseModels,
  },
}));

mock.module('../../../features/auth', () => ({
  useAuth: () => ({
    isAuthenticated: authState.isAuthenticated,
  }),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

let useBrowse: typeof import('./useBrowse').useBrowse;

beforeAll(async () => {
  ({ useBrowse } = await import('./useBrowse'));
});

afterAll(() => {
  mock.restore();
});

describe('useBrowse', () => {
  beforeEach(() => {
    mockBrowseModels.mockReset();
    // Reset auth state to default
    authState = {
      isAuthenticated: true,
    };
  });

  it('initializes with default state', () => {
    const { result } = renderHook(() => useBrowse());

    expect(result.current.source).toBe('all');
    expect(result.current.query).toBe('');
    expect(result.current.models).toEqual([]);
    expect(result.current.loading).toBe(false);
    expect(result.current.loadingMore).toBe(false);
    expect(result.current.hasMore).toBe(true);
    expect(result.current.error).toBeNull();
  });

  it('searches models from single source when authenticated', async () => {
    const mockModels = [
      { name: 'llama2:7b', size: 3800000000, details: { family: 'llama', parameter_size: '7B' } },
    ];

    // Mock for changeSource call
    mockBrowseModels.mockResolvedValueOnce({
      models: mockModels,
      next_cursor: null,
    });

    const { result } = renderHook(() => useBrowse());

    // Change to single source - this triggers a search
    await act(async () => {
      result.current.changeSource('ollama');
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(result.current.source).toBe('ollama');
    });

    // Models should have source field added
    expect(result.current.models).toEqual(mockModels.map(m => ({ ...m, source: 'ollama' })));
    expect(result.current.hasMore).toBe(false); // next_cursor is null
    expect(mockBrowseModels).toHaveBeenCalledWith('ollama', '', null, 20);
  });

  it('searches all sources when source is all', async () => {
    const ollamaModels = [{ name: 'llama:7b', size: 1000000000 }];
    const huggingfaceModels = [{ name: 'hf/model', size: 2000000000 }];

    // Mock responses for each source - all 4 sources will be called
    mockBrowseModels
      .mockResolvedValueOnce({ models: ollamaModels, next_cursor: null })
      .mockResolvedValueOnce({ models: huggingfaceModels, next_cursor: null })
      .mockResolvedValueOnce({ models: [], next_cursor: null })
      .mockResolvedValueOnce({ models: [], next_cursor: null });

    const { result } = renderHook(() => useBrowse());

    await act(async () => {
      await result.current.search('', 'all');
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    // Should have interleaved results from both sources
    expect(result.current.models.length).toBe(2);
    // Models should have source field set
    expect(result.current.models.some(m => m.source === 'ollama')).toBe(true);
    expect(result.current.models.some(m => m.source === 'huggingface')).toBe(true);
  });

  it('does not search when not authenticated', async () => {
    authState = { isAuthenticated: false };

    const { result } = renderHook(() => useBrowse());

    await act(async () => {
      await result.current.search('llama', 'ollama');
    });

    expect(mockBrowseModels).not.toHaveBeenCalled();
  });

  it('handles search error for single source', async () => {
    mockBrowseModels.mockRejectedValueOnce(new Error('Network error'));

    const { result } = renderHook(() => useBrowse());

    // Change to single source first with a rejected promise
    await act(async () => {
      result.current.changeSource('ollama');
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.error).toBe('Network error');
    expect(result.current.models).toEqual([]);
  });

  it('loads more models using cursor for single source', async () => {
    // Create arrays of 20 items to trigger hasMore=true
    const firstPage = Array.from({ length: 20 }, (_, i) => ({
      name: `model-${i}`,
      size: 1000000000,
    }));
    const secondPage = [{ name: 'model-extra', size: 1000000000 }];

    // First mock for changeSource call, second for loadMore
    mockBrowseModels
      .mockResolvedValueOnce({ models: firstPage, next_cursor: 'cursor-page-2' })
      .mockResolvedValueOnce({ models: secondPage, next_cursor: null });

    const { result } = renderHook(() => useBrowse());

    // Switch to single source - this triggers search
    await act(async () => {
      result.current.changeSource('ollama');
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(result.current.models).toHaveLength(20);
    });

    expect(result.current.hasMore).toBe(true); // has cursor for next page

    await act(async () => {
      await result.current.loadMore();
    });

    await waitFor(() => {
      expect(result.current.loadingMore).toBe(false);
    });

    expect(result.current.models).toHaveLength(21);
    expect(result.current.hasMore).toBe(false); // next_cursor is null
    // Verify cursor was passed in loadMore call
    expect(mockBrowseModels).toHaveBeenLastCalledWith('ollama', '', 'cursor-page-2', 20);
  });

  it('does not load more when no more results available', async () => {
    mockBrowseModels.mockResolvedValueOnce({
      models: [],
      next_cursor: null,
    });

    const { result } = renderHook(() => useBrowse());

    // Change to single source
    await act(async () => {
      result.current.changeSource('ollama');
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(result.current.hasMore).toBe(false);
    });

    // Try to load more - should not call API
    const callsBefore = mockBrowseModels.mock.calls.length;
    await act(async () => {
      await result.current.loadMore();
    });

    // Should not have made additional calls
    expect(mockBrowseModels).toHaveBeenCalledTimes(callsBefore);
  });

  it('changes source and clears results', async () => {
    mockBrowseModels
      .mockResolvedValueOnce({ models: [{ name: 'ollama-model', size: 1000000000 }], next_cursor: null })
      .mockResolvedValueOnce({ models: [{ name: 'hf-model', size: 2000000000 }], next_cursor: null });

    const { result } = renderHook(() => useBrowse());

    // Change to ollama source
    await act(async () => {
      result.current.changeSource('ollama');
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(result.current.models).toHaveLength(1);
    });

    await act(async () => {
      result.current.changeSource('huggingface');
    });

    await waitFor(() => {
      expect(result.current.source).toBe('huggingface');
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.query).toBe('');
    expect(mockBrowseModels).toHaveBeenLastCalledWith('huggingface', '', null, 20);
  });

  it('sets query', () => {
    const { result } = renderHook(() => useBrowse());

    act(() => {
      result.current.setQuery('test query');
    });

    expect(result.current.query).toBe('test query');
  });

  it('handles loadMore error for single source', async () => {
    const firstPage = Array.from({ length: 20 }, (_, i) => ({
      name: `model-${i}`,
      size: 1000000000,
    }));

    mockBrowseModels
      .mockResolvedValueOnce({ models: firstPage, next_cursor: 'next-page' })
      .mockRejectedValueOnce(new Error('Load more failed'));

    const { result } = renderHook(() => useBrowse());

    // Change to single source
    await act(async () => {
      result.current.changeSource('ollama');
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(result.current.models).toHaveLength(20);
    });

    await act(async () => {
      await result.current.loadMore();
    });

    await waitFor(() => {
      expect(result.current.loadingMore).toBe(false);
    });

    expect(result.current.error).toBe('Load more failed');
    // Original models should still be there
    expect(result.current.models).toHaveLength(20);
  });

  it('handles non-Error object in search error', async () => {
    mockBrowseModels.mockRejectedValueOnce('String error');

    const { result } = renderHook(() => useBrowse());

    // Change to single source - triggers search with rejection
    await act(async () => {
      result.current.changeSource('ollama');
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.error).toBe('Failed to browse models');
    expect(result.current.models).toEqual([]);
  });

  it('handles non-Error object in loadMore error', async () => {
    const firstPage = Array.from({ length: 20 }, (_, i) => ({
      name: `model-${i}`,
      size: 1000000000,
    }));

    mockBrowseModels
      .mockResolvedValueOnce({ models: firstPage, next_cursor: 'next-page' })
      .mockRejectedValueOnce('String error');

    const { result } = renderHook(() => useBrowse());

    // Change to single source
    await act(async () => {
      result.current.changeSource('ollama');
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(result.current.models).toHaveLength(20);
    });

    await act(async () => {
      await result.current.loadMore();
    });

    await waitFor(() => {
      expect(result.current.loadingMore).toBe(false);
    });

    expect(result.current.error).toBe('Failed to load more models');
    expect(result.current.models).toHaveLength(20);
  });
});
