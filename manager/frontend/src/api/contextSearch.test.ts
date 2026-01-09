import { client } from './client';

describe('Context Search API', () => {
  beforeEach(() => {
    global.fetch = jest.fn();
    global.WebSocket = jest.fn() as unknown as typeof WebSocket;
  });

  afterEach(() => {
    jest.resetAllMocks();
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

      (global.fetch as jest.Mock).mockResolvedValue({
        ok: true,
        json: async () => mockResponse,
      });

      const result = await client.searchContext({ query: 'test query' });

      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/context/search?query=test+query'),
        expect.objectContaining({
          headers: expect.objectContaining({
            'Content-Type': 'application/json',
          }),
        })
      );
      expect(result).toEqual(mockResponse);
    });

    it('should search context with mode parameter', async () => {
      const mockResponse = { results: [], total: 0 };

      (global.fetch as jest.Mock).mockResolvedValue({
        ok: true,
        json: async () => mockResponse,
      });

      await client.searchContext({ query: 'test', mode: 'semantic' });

      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining('mode=semantic'),
        expect.any(Object)
      );
    });

    it('should search context with source_ids filter', async () => {
      const mockResponse = { results: [], total: 0 };

      (global.fetch as jest.Mock).mockResolvedValue({
        ok: true,
        json: async () => mockResponse,
      });

      await client.searchContext({ query: 'test', source_ids: ['s1', 's2'] });

      const call = (global.fetch as jest.Mock).mock.calls[0][0];
      expect(call).toContain('source_ids=s1');
      expect(call).toContain('source_ids=s2');
    });

    it('should search context with limit', async () => {
      const mockResponse = { results: [], total: 0 };

      (global.fetch as jest.Mock).mockResolvedValue({
        ok: true,
        json: async () => mockResponse,
      });

      await client.searchContext({ query: 'test', limit: 50 });

      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining('limit=50'),
        expect.any(Object)
      );
    });

    it('should URL encode query parameters', async () => {
      const mockResponse = { results: [], total: 0 };

      (global.fetch as jest.Mock).mockResolvedValue({
        ok: true,
        json: async () => mockResponse,
      });

      await client.searchContext({ query: 'test query with spaces & symbols' });

      const call = (global.fetch as jest.Mock).mock.calls[0][0];
      expect(call).toContain('query=test+query+with+spaces+%26+symbols');
    });

    it('should throw error on failed request', async () => {
      (global.fetch as jest.Mock).mockResolvedValue({
        ok: false,
        status: 500,
        json: async () => ({ message: 'Server error' }),
      });

      await expect(client.searchContext({ query: 'test' })).rejects.toThrow('Server error');
    });
  });

  describe('gatherContext', () => {
    it('should gather context from sources', async () => {
      const mockResponse = { gathering_id: 'g123' };

      (global.fetch as jest.Mock).mockResolvedValue({
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
      (global.fetch as jest.Mock).mockResolvedValue({
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

      const mockWebSocket = global.WebSocket as unknown as jest.Mock;
      const call = mockWebSocket.mock.calls[0][0];
      expect(call).toContain('/ws/context/g123');
    });

    it('should URL encode gathering ID', () => {
      const ws = client.createContextGatheringWebSocket('g/123');

      const mockWebSocket = global.WebSocket as unknown as jest.Mock;
      const call = mockWebSocket.mock.calls[0][0];
      expect(call).toContain('g%2F123');
    });
  });
});
