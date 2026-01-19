import { act, renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import type { SearchResult, SearchOptions } from '../types';
import { createElement } from 'react';
import type { ReactNode } from 'react';

const mockSearchContext = mock();

mock.module('../../../api/knowledge', () => ({
  knowledgeApi: {
    searchContext: mockSearchContext,
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

let useContextSearch: typeof import('./useContextSearch').useContextSearch;

beforeAll(async () => {
  ({ useContextSearch } = await import('./useContextSearch'));
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

describe('useContextSearch', () => {
  const mockResults: SearchResult[] = [
    {
      id: 'r1',
      source_id: 's1',
      source_name: 'Test Source',
      content: 'Full content',
      snippet: 'Test snippet',
      relevance_score: 0.95,
      metadata: { type: 'file', path: '/test.ts' },
    },
    {
      id: 'r2',
      source_id: 's2',
      source_name: 'Another Source',
      content: 'Another content',
      snippet: 'Another snippet',
      relevance_score: 0.75,
      metadata: {},
    },
  ];

  beforeEach(() => {
    mockSearchContext.mockReset();
  });

  describe('initialization', () => {
    it('should initialize with empty state', () => {
      const { result } = renderHook(() => useContextSearch(), { wrapper: createWrapper() });

      expect(result.current.results).toEqual([]);
      expect(result.current.total).toBe(0);
      expect(result.current.loading).toBe(false);
      expect(result.current.error).toBeNull();
    });
  });

  describe('search', () => {
    it('should perform search with query', async () => {
      mockSearchContext.mockResolvedValue({
        results: mockResults,
        total: 2,
      });

      const { result } = renderHook(() => useContextSearch(), { wrapper: createWrapper() });

      const options: SearchOptions = {
        query: 'test query',
        mode: 'hybrid',
        limit: 20,
      };

      await act(async () => {
        await result.current.search(options);
      });

      expect(mockSearchContext).toHaveBeenCalledWith({ ...options, workspace_id: 'test-workspace-id' });
      expect(result.current.results).toEqual(mockResults);
      expect(result.current.total).toBe(2);
      expect(result.current.loading).toBe(false);
      expect(result.current.error).toBeNull();
    });

    it('should set loading state during search', async () => {
      mockSearchContext.mockImplementation(
        () =>
          new Promise((resolve) =>
            setTimeout(() => resolve({ results: mockResults, total: 2 }), 100)
          )
      );

      const { result } = renderHook(() => useContextSearch(), { wrapper: createWrapper() });

      const options: SearchOptions = {
        query: 'test query',
      };

      act(() => {
        result.current.search(options);
      });

      expect(result.current.loading).toBe(true);

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      expect(result.current.results).toEqual(mockResults);
    });

    it('should handle search error', async () => {
      mockSearchContext.mockRejectedValue(new Error('Search failed'));

      const { result } = renderHook(() => useContextSearch(), { wrapper: createWrapper() });

      const options: SearchOptions = {
        query: 'test query',
      };

      await act(async () => {
        await result.current.search(options);
      });

      expect(result.current.error).toBe('Search failed');
      expect(result.current.results).toEqual([]);
      expect(result.current.total).toBe(0);
      expect(result.current.loading).toBe(false);
    });

    it('should search with semantic mode', async () => {
      mockSearchContext.mockResolvedValue({
        results: mockResults,
        total: 2,
      });

      const { result } = renderHook(() => useContextSearch(), { wrapper: createWrapper() });

      const options: SearchOptions = {
        query: 'test query',
        mode: 'semantic',
      };

      await act(async () => {
        await result.current.search(options);
      });

      expect(mockSearchContext).toHaveBeenCalledWith(
        expect.objectContaining({ mode: 'semantic' })
      );
    });

    it('should search with keyword mode', async () => {
      mockSearchContext.mockResolvedValue({
        results: mockResults,
        total: 2,
      });

      const { result } = renderHook(() => useContextSearch(), { wrapper: createWrapper() });

      const options: SearchOptions = {
        query: 'test query',
        mode: 'keyword',
      };

      await act(async () => {
        await result.current.search(options);
      });

      expect(mockSearchContext).toHaveBeenCalledWith(
        expect.objectContaining({ mode: 'keyword' })
      );
    });

    it('should search with source filters', async () => {
      mockSearchContext.mockResolvedValue({
        results: mockResults,
        total: 2,
      });

      const { result } = renderHook(() => useContextSearch(), { wrapper: createWrapper() });

      const options: SearchOptions = {
        query: 'test query',
        source_ids: ['s1', 's2'],
      };

      await act(async () => {
        await result.current.search(options);
      });

      expect(mockSearchContext).toHaveBeenCalledWith(
        expect.objectContaining({ source_ids: ['s1', 's2'] })
      );
    });

    it('should search with custom limit', async () => {
      mockSearchContext.mockResolvedValue({
        results: mockResults,
        total: 2,
      });

      const { result } = renderHook(() => useContextSearch(), { wrapper: createWrapper() });

      const options: SearchOptions = {
        query: 'test query',
        limit: 10,
      };

      await act(async () => {
        await result.current.search(options);
      });

      expect(mockSearchContext).toHaveBeenCalledWith(
        expect.objectContaining({ limit: 10 })
      );
    });

    it('should clear previous results on new search', async () => {
      mockSearchContext.mockResolvedValueOnce({
        results: mockResults,
        total: 2,
      });

      const { result } = renderHook(() => useContextSearch(), { wrapper: createWrapper() });

      await act(async () => {
        await result.current.search({ query: 'first query' });
      });

      expect(result.current.results).toEqual(mockResults);

      const newResults = [mockResults[0]];
      mockSearchContext.mockResolvedValueOnce({
        results: newResults,
        total: 1,
      });

      await act(async () => {
        await result.current.search({ query: 'second query' });
      });

      expect(result.current.results).toEqual(newResults);
      expect(result.current.total).toBe(1);
    });
  });

  describe('clear', () => {
    it('should clear search results', async () => {
      mockSearchContext.mockResolvedValue({
        results: mockResults,
        total: 2,
      });

      const { result } = renderHook(() => useContextSearch(), { wrapper: createWrapper() });

      await act(async () => {
        await result.current.search({ query: 'test query' });
      });

      expect(result.current.results).toEqual(mockResults);
      expect(result.current.total).toBe(2);

      act(() => {
        result.current.clear();
      });

      expect(result.current.results).toEqual([]);
      expect(result.current.total).toBe(0);
      expect(result.current.error).toBeNull();
    });
  });
});
