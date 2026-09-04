import { afterAll, afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { client } from './client';

describe('Context Search API', () => {
  const testWorkspaceId = 'ws-test-123';
  // Every test file shares one process, so a global left swapped out here
  // reaches suites that expect a real WebSocket.
  const realWebSocket = global.WebSocket;
  let mockFetch: ReturnType<typeof mock>;
  let mockWebSocket: ReturnType<typeof mock>;

  beforeEach(() => {
    mockFetch = mock();
    mockWebSocket = mock();
    global.fetch = mockFetch as typeof fetch;
    global.WebSocket = mockWebSocket as unknown as typeof WebSocket;
  });

  afterEach(() => {
    mock.clearAllMocks();
  });

  afterAll(() => {
    global.WebSocket = realWebSocket;
  });

  describe('searchContext', () => {
    it('should search context with basic query', async () => {
      const mockResponse = {
        results: [
          {
            id: '1',
            source_id: 's1',
            source_name: 'Test Source',
            content: 'Full content here',
            snippet: 'Highlighted snippet',
            relevance_score: 0.95,
            metadata: { type: 'file' },
          },
        ],
        total: 1,
      };

      mockFetch.mockResolvedValue({
        ok: true,
        json: async () => mockResponse,
      });

      const result = await client.searchContext({
        workspace_id: testWorkspaceId,
        query: 'test query',
      });

      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/context/search?'),
        expect.objectContaining({
          headers: expect.objectContaining({
            'Content-Type': 'application/json',
          }),
        })
      );
      const call = mockFetch.mock.calls[0][0];
      expect(call).toContain(`workspace_id=${testWorkspaceId}`);
      expect(call).toContain('q=test+query');
      expect(result).toEqual(mockResponse);
    });

    it('should search context with mode parameter', async () => {
      const mockResponse = { results: [], total: 0 };

      mockFetch.mockResolvedValue({
        ok: true,
        json: async () => mockResponse,
      });

      await client.searchContext({
        workspace_id: testWorkspaceId,
        query: 'test',
        mode: 'semantic',
      });

      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining('mode=semantic'),
        expect.any(Object)
      );
    });

    it('should search context with source_ids filter', async () => {
      const mockResponse = { results: [], total: 0 };

      mockFetch.mockResolvedValue({
        ok: true,
        json: async () => mockResponse,
      });

      await client.searchContext({
        workspace_id: testWorkspaceId,
        query: 'test',
        source_ids: ['s1', 's2'],
      });

      const call = mockFetch.mock.calls[0][0];
      expect(call).toContain('source_ids=s1');
      expect(call).toContain('source_ids=s2');
    });

    it('should search context with limit', async () => {
      const mockResponse = { results: [], total: 0 };

      mockFetch.mockResolvedValue({
        ok: true,
        json: async () => mockResponse,
      });

      await client.searchContext({ workspace_id: testWorkspaceId, query: 'test', limit: 50 });

      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining('limit=50'),
        expect.any(Object)
      );
    });

    it('should URL encode query parameters', async () => {
      const mockResponse = { results: [], total: 0 };

      mockFetch.mockResolvedValue({
        ok: true,
        json: async () => mockResponse,
      });

      await client.searchContext({
        workspace_id: testWorkspaceId,
        query: 'test query with spaces & symbols',
      });

      const call = mockFetch.mock.calls[0][0];
      expect(call).toContain('q=test+query+with+spaces+%26+symbols');
    });

    it('should throw error on failed request', async () => {
      mockFetch.mockResolvedValue({
        ok: false,
        status: 500,
        json: async () => ({ message: 'Server error' }),
      });

      await expect(
        client.searchContext({ workspace_id: testWorkspaceId, query: 'test' })
      ).rejects.toThrow('Server error');
    });
  });

  describe('gatherContext', () => {
    it('should gather context from sources', async () => {
      const mockResponse = { gathering_id: 'g123' };

      mockFetch.mockResolvedValue({
        ok: true,
        json: async () => mockResponse,
      });

      const result = await client.gatherContext({ source_ids: ['s1', 's2'] });

      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/context/gather'),
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ source_ids: ['s1', 's2'] }),
        })
      );
      expect(result).toEqual(mockResponse);
    });

    it('should throw error on failed gather', async () => {
      mockFetch.mockResolvedValue({
        ok: false,
        status: 400,
        json: async () => ({ message: 'Invalid request' }),
      });

      await expect(client.gatherContext({ source_ids: [] })).rejects.toThrow('Invalid request');
    });
  });

  describe('createContextGatheringWebSocket', () => {
    it('should create WebSocket with correct URL', () => {
      const ws = client.createContextGatheringWebSocket('g123');

      const call = mockWebSocket.mock.calls[0][0];
      expect(call).toContain('/ws/context/g123');
    });

    it('should URL encode gathering ID', () => {
      const ws = client.createContextGatheringWebSocket('g/123');

      const call = mockWebSocket.mock.calls[0][0];
      expect(call).toContain('g%2F123');
    });
  });
});
