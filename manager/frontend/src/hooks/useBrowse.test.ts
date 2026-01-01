import { renderHook, waitFor, act } from '@testing-library/react';
import { useBrowse } from './useBrowse';
import { client } from '../api/client';

// Mock the client
jest.mock('../api/client', () => ({
  client: {
    browseModels: jest.fn(),
  },
}));

// Mock useAuth hook
jest.mock('../context/AuthContext', () => ({
  useAuth: jest.fn(() => ({
    isAuthenticated: true,
  })),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

import { useAuth } from '../context/AuthContext';

const mockClient = client as jest.Mocked<typeof client>;
const mockUseAuth = useAuth as jest.Mock;

describe('useBrowse', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    mockUseAuth.mockReturnValue({
      isAuthenticated: true,
    });
  });

  it('initializes with default state', () => {
    const { result } = renderHook(() => useBrowse());

    expect(result.current.source).toBe('ollama');
    expect(result.current.query).toBe('');
    expect(result.current.models).toEqual([]);
    expect(result.current.loading).toBe(false);
    expect(result.current.loadingMore).toBe(false);
    expect(result.current.hasMore).toBe(true);
    expect(result.current.error).toBeNull();
  });

  it('searches models when authenticated', async () => {
    const mockModels = [
      { id: 'model1', name: 'llama2', description: 'A model' },
    ];
    mockClient.browseModels.mockResolvedValueOnce({
      models: mockModels,
      has_more: false,
    });

    const { result } = renderHook(() => useBrowse());

    await act(async () => {
      await result.current.search('llama');
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.models).toEqual(mockModels);
    expect(result.current.hasMore).toBe(false);
    expect(mockClient.browseModels).toHaveBeenCalledWith('ollama', 'llama', 0, 20);
  });

  it('does not search when not authenticated', async () => {
    mockUseAuth.mockReturnValue({
      isAuthenticated: false,
    });

    const { result } = renderHook(() => useBrowse());

    await act(async () => {
      await result.current.search('llama');
    });

    expect(mockClient.browseModels).not.toHaveBeenCalled();
  });

  it('handles search error', async () => {
    mockClient.browseModels.mockRejectedValueOnce(new Error('Network error'));

    const { result } = renderHook(() => useBrowse());

    await act(async () => {
      await result.current.search('llama');
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.error).toBe('Network error');
    expect(result.current.models).toEqual([]);
  });

  it('loads more models', async () => {
    const firstPage = [{ id: '1', name: 'model1', description: '' }];
    const secondPage = [{ id: '2', name: 'model2', description: '' }];

    mockClient.browseModels
      .mockResolvedValueOnce({ models: firstPage, has_more: true })
      .mockResolvedValueOnce({ models: secondPage, has_more: false });

    const { result } = renderHook(() => useBrowse());

    await act(async () => {
      await result.current.search();
    });

    await waitFor(() => {
      expect(result.current.models).toHaveLength(1);
    });

    await act(async () => {
      await result.current.loadMore();
    });

    await waitFor(() => {
      expect(result.current.loadingMore).toBe(false);
    });

    expect(result.current.models).toHaveLength(2);
    expect(result.current.hasMore).toBe(false);
  });

  it('does not load more when no more results available', async () => {
    mockClient.browseModels.mockResolvedValueOnce({ models: [], has_more: false });

    const { result } = renderHook(() => useBrowse());

    await act(async () => {
      await result.current.search();
    });

    await waitFor(() => {
      expect(result.current.hasMore).toBe(false);
    });

    // Try to load more - should not call API
    const callsBefore = mockClient.browseModels.mock.calls.length;
    await act(async () => {
      await result.current.loadMore();
    });

    // Should not have made additional calls
    expect(mockClient.browseModels).toHaveBeenCalledTimes(callsBefore);
  });


  it('changes source and clears results', async () => {
    mockClient.browseModels
      .mockResolvedValueOnce({ models: [{ id: '1', name: 'ollama-model', description: '' }], has_more: false })
      .mockResolvedValueOnce({ models: [{ id: '2', name: 'hf-model', description: '' }], has_more: false });

    const { result } = renderHook(() => useBrowse());

    await act(async () => {
      await result.current.search();
    });

    await waitFor(() => {
      expect(result.current.models).toHaveLength(1);
    });

    await act(async () => {
      result.current.changeSource('huggingface');
    });

    await waitFor(() => {
      expect(result.current.source).toBe('huggingface');
    });

    expect(result.current.query).toBe('');
    expect(mockClient.browseModels).toHaveBeenLastCalledWith('huggingface', '', 0, 20);
  });

  it('sets query', () => {
    const { result } = renderHook(() => useBrowse());

    act(() => {
      result.current.setQuery('test query');
    });

    expect(result.current.query).toBe('test query');
  });

  it('handles loadMore error', async () => {
    mockClient.browseModels
      .mockResolvedValueOnce({ models: [{ id: '1', name: 'model', description: '' }], has_more: true })
      .mockRejectedValueOnce(new Error('Load more failed'));

    const { result } = renderHook(() => useBrowse());

    await act(async () => {
      await result.current.search();
    });

    await act(async () => {
      await result.current.loadMore();
    });

    await waitFor(() => {
      expect(result.current.loadingMore).toBe(false);
    });

    expect(result.current.error).toBe('Load more failed');
    // Original models should still be there
    expect(result.current.models).toHaveLength(1);
  });

  it('handles non-Error object in search error', async () => {
    mockClient.browseModels.mockRejectedValueOnce('String error');

    const { result } = renderHook(() => useBrowse());

    await act(async () => {
      await result.current.search('llama');
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.error).toBe('Failed to browse models');
    expect(result.current.models).toEqual([]);
  });

  it('handles non-Error object in loadMore error', async () => {
    mockClient.browseModels
      .mockResolvedValueOnce({ models: [{ id: '1', name: 'model', description: '' }], has_more: true })
      .mockRejectedValueOnce('String error');

    const { result } = renderHook(() => useBrowse());

    await act(async () => {
      await result.current.search();
    });

    await act(async () => {
      await result.current.loadMore();
    });

    await waitFor(() => {
      expect(result.current.loadingMore).toBe(false);
    });

    expect(result.current.error).toBe('Failed to load more models');
    expect(result.current.models).toHaveLength(1);
  });
});
