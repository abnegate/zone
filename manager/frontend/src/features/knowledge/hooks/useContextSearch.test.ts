import { act, renderHook, waitFor } from '@testing-library/react';
import { knowledgeApi } from '../../../api/knowledge';
import { useContextSearch } from './useContextSearch';
import type { SearchResult, SearchOptions } from '../types';

jest.mock('../../../api/knowledge');

const mockKnowledgeApi = knowledgeApi as jest.Mocked<typeof knowledgeApi>;

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
    jest.clearAllMocks();
  });

  describe('initialization', () => {
    it('should initialize with empty state', () => {
      const { result } = renderHook(() => useContextSearch());

      expect(result.current.results).toEqual([]);
      expect(result.current.total).toBe(0);
      expect(result.current.loading).toBe(false);
      expect(result.current.error).toBeNull();
    });
  });

  describe('search', () => {
    it('should perform search with query', async () => {
      mockKnowledgeApi.searchContext.mockResolvedValue({
        results: mockResults,
        total: 2,
      });

      const { result } = renderHook(() => useContextSearch());

      const options: SearchOptions = {
        query: 'test query',
        mode: 'hybrid',
        limit: 20,
      };

      await act(async () => {
        await result.current.search(options);
      });

      expect(mockKnowledgeApi.searchContext).toHaveBeenCalledWith(options);
      expect(result.current.results).toEqual(mockResults);
      expect(result.current.total).toBe(2);
      expect(result.current.loading).toBe(false);
      expect(result.current.error).toBeNull();
    });

    it('should set loading state during search', async () => {
      mockKnowledgeApi.searchContext.mockImplementation(
        () =>
          new Promise((resolve) =>
            setTimeout(() => resolve({ results: mockResults, total: 2 }), 100)
          )
      );

      const { result } = renderHook(() => useContextSearch());

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
      mockKnowledgeApi.searchContext.mockRejectedValue(new Error('Search failed'));

      const { result } = renderHook(() => useContextSearch());

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
      mockKnowledgeApi.searchContext.mockResolvedValue({
        results: mockResults,
        total: 2,
      });

      const { result } = renderHook(() => useContextSearch());

      const options: SearchOptions = {
        query: 'test query',
        mode: 'semantic',
      };

      await act(async () => {
        await result.current.search(options);
      });

      expect(mockKnowledgeApi.searchContext).toHaveBeenCalledWith(
        expect.objectContaining({ mode: 'semantic' })
      );
    });

    it('should search with keyword mode', async () => {
      mockKnowledgeApi.searchContext.mockResolvedValue({
        results: mockResults,
        total: 2,
      });

      const { result } = renderHook(() => useContextSearch());

      const options: SearchOptions = {
        query: 'test query',
        mode: 'keyword',
      };

      await act(async () => {
        await result.current.search(options);
      });

      expect(mockKnowledgeApi.searchContext).toHaveBeenCalledWith(
        expect.objectContaining({ mode: 'keyword' })
      );
    });

    it('should search with source filters', async () => {
      mockKnowledgeApi.searchContext.mockResolvedValue({
        results: mockResults,
        total: 2,
      });

      const { result } = renderHook(() => useContextSearch());

      const options: SearchOptions = {
        query: 'test query',
        source_ids: ['s1', 's2'],
      };

      await act(async () => {
        await result.current.search(options);
      });

      expect(mockKnowledgeApi.searchContext).toHaveBeenCalledWith(
        expect.objectContaining({ source_ids: ['s1', 's2'] })
      );
    });

    it('should search with custom limit', async () => {
      mockKnowledgeApi.searchContext.mockResolvedValue({
        results: mockResults,
        total: 2,
      });

      const { result } = renderHook(() => useContextSearch());

      const options: SearchOptions = {
        query: 'test query',
        limit: 10,
      };

      await act(async () => {
        await result.current.search(options);
      });

      expect(mockKnowledgeApi.searchContext).toHaveBeenCalledWith(
        expect.objectContaining({ limit: 10 })
      );
    });

    it('should clear previous results on new search', async () => {
      mockKnowledgeApi.searchContext.mockResolvedValueOnce({
        results: mockResults,
        total: 2,
      });

      const { result } = renderHook(() => useContextSearch());

      await act(async () => {
        await result.current.search({ query: 'first query' });
      });

      expect(result.current.results).toEqual(mockResults);

      const newResults = [mockResults[0]];
      mockKnowledgeApi.searchContext.mockResolvedValueOnce({
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
      mockKnowledgeApi.searchContext.mockResolvedValue({
        results: mockResults,
        total: 2,
      });

      const { result } = renderHook(() => useContextSearch());

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
