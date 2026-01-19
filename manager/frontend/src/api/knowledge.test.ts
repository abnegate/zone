import { afterEach, beforeEach, describe, expect, it, spyOn } from 'bun:test';
import { knowledgeApi } from './knowledge';
import type { CreateKnowledgeRequest, GatherContextRequest } from '../features/knowledge/types';

describe('KnowledgeApi', () => {
  let mockFetch: ReturnType<typeof spyOn>;

  beforeEach(() => {
    mockFetch = spyOn(global, 'fetch');
    knowledgeApi.setGetAccessToken(() => 'test-token');
  });

  afterEach(() => {
    mockFetch.mockRestore();
  });

  // =============================================================================
  // Knowledge Base Tests
  // =============================================================================

  describe('getKnowledge', () => {
    it('should fetch knowledge entries with validation', async () => {
      const mockResponse = {
        entries: [
          {
            id: 'k1',
            workspace_id: 'w1',
            title: 'Test Knowledge',
            type: 'text',
            content: 'Test content',
            fetched_content: null,
            tags: ['tag1'],
            last_refreshed_at: null,
            created_at: '2024-01-01T00:00:00Z',
            updated_at: '2024-01-01T00:00:00Z',
          },
        ],
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockResponse,
      });

      const result = await knowledgeApi.getKnowledge('w1');
      expect(result).toEqual(mockResponse);
      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/knowledge?workspace_id=w1'),
        expect.objectContaining({
          headers: expect.objectContaining({
            Authorization: 'Bearer test-token',
          }),
        })
      );
    });

    it('should reject invalid knowledge response', async () => {
      const invalidResponse = {
        entries: [
          {
            id: '', // Invalid: empty string
            workspace_id: 'w1',
            title: 'Test',
          },
        ],
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => invalidResponse,
      });

      await expect(knowledgeApi.getKnowledge()).rejects.toThrow();
    });
  });

  describe('createKnowledge', () => {
    it('should create knowledge with validation', async () => {
      const request: CreateKnowledgeRequest = {
        title: 'New Knowledge',
        type: 'text',
        content: 'Test content',
        tags: ['tag1'],
      };

      const mockResponse = {
        id: 'k1',
        workspace_id: 'w1',
        title: 'New Knowledge',
        type: 'text',
        content: 'Test content',
        fetched_content: null,
        tags: ['tag1'],
        last_refreshed_at: null,
        created_at: '2024-01-01T00:00:00Z',
        updated_at: '2024-01-01T00:00:00Z',
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockResponse,
      });

      const result = await knowledgeApi.createKnowledge(request);
      expect(result).toEqual(mockResponse);
    });
  });

  // =============================================================================
  // Context Search Tests
  // =============================================================================

  describe('searchContext', () => {
    it('should search context with validation', async () => {
      const mockResponse = {
        results: [
          {
            id: 'r1',
            source_id: 's1',
            source_name: 'Test Source',
            content: 'Full content',
            snippet: 'Test snippet',
            relevance_score: 0.9,
            metadata: { type: 'file' },
          },
        ],
        total: 1,
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockResponse,
      });

      const result = await knowledgeApi.searchContext({
        query: 'test',
        mode: 'hybrid',
        limit: 10,
      });

      expect(result).toEqual(mockResponse);
    });

    it('should reject invalid search response', async () => {
      const invalidResponse = {
        results: [
          {
            id: 'r1',
            source_id: 's1',
            source_name: 'Test',
            content: 'Content',
            snippet: 'Snippet',
            relevance_score: 1.5, // Invalid: > 1
            metadata: {},
          },
        ],
        total: 1,
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => invalidResponse,
      });

      await expect(knowledgeApi.searchContext({ query: 'test' })).rejects.toThrow();
    });
  });

  describe('gatherContext', () => {
    it('should gather context with response validation', async () => {
      const request: GatherContextRequest = {
        source_ids: ['s1', 's2'],
      };

      const mockResponse = {
        gathering_id: 'g123',
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockResponse,
      });

      const result = await knowledgeApi.gatherContext(request);
      expect(result).toEqual(mockResponse);
      expect(result.gathering_id).toBe('g123');
    });

    it('should reject invalid gather context response', async () => {
      const request: GatherContextRequest = {
        source_ids: ['s1'],
      };

      const invalidResponse = {
        gathering_id: '', // Invalid: empty string
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => invalidResponse,
      });

      await expect(knowledgeApi.gatherContext(request)).rejects.toThrow();
    });

    it('should reject response missing gathering_id', async () => {
      const request: GatherContextRequest = {
        source_ids: ['s1'],
      };

      const invalidResponse = {
        // Missing gathering_id
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => invalidResponse,
      });

      await expect(knowledgeApi.gatherContext(request)).rejects.toThrow();
    });
  });

  describe('refreshKnowledge', () => {
    it('should refresh knowledge with validation', async () => {
      const mockResponse = {
        id: 'k1',
        workspace_id: 'w1',
        title: 'Refreshed Knowledge',
        type: 'url',
        content: 'https://example.com',
        fetched_content: 'Updated content',
        tags: [],
        last_refreshed_at: '2024-01-02T00:00:00Z',
        created_at: '2024-01-01T00:00:00Z',
        updated_at: '2024-01-02T00:00:00Z',
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockResponse,
      });

      const result = await knowledgeApi.refreshKnowledge('k1');
      expect(result).toEqual(mockResponse);
    });
  });

  // =============================================================================
  // Error Handling Tests
  // =============================================================================

  describe('error handling', () => {
    it('should handle HTTP errors', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 404,
        json: async () => ({ message: 'Not found' }),
      });

      await expect(knowledgeApi.getKnowledge()).rejects.toThrow('Not found');
    });

    it('should handle network errors', async () => {
      mockFetch.mockRejectedValueOnce(new Error('Network error'));

      await expect(knowledgeApi.getKnowledge()).rejects.toThrow('Network error');
    });

    it('should handle malformed error responses', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 500,
        json: async () => {
          throw new Error('Invalid JSON');
        },
      });

      await expect(knowledgeApi.getKnowledge()).rejects.toThrow();
    });
  });
});
