import { act, renderHook, waitFor } from '@testing-library/react';
import { knowledgeApi } from '../../../api/knowledge';
import { useKnowledge } from './useKnowledge';
import type { KnowledgeEntry, CreateKnowledgeRequest } from '../types';

jest.mock('../../../api/knowledge');

const mockKnowledgeApi = knowledgeApi as jest.Mocked<typeof knowledgeApi>;

describe('useKnowledge', () => {
  const mockEntries: KnowledgeEntry[] = [
    {
      id: 'kb-1',
      workspace_id: 'ws-1',
      title: 'Test Entry',
      type: 'text',
      content: 'Test content',
      fetched_content: null,
      tags: ['test'],
      last_refreshed_at: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    },
    {
      id: 'kb-2',
      workspace_id: 'ws-1',
      title: 'URL Entry',
      type: 'url',
      content: 'https://example.com',
      fetched_content: 'Fetched content',
      tags: [],
      last_refreshed_at: '2024-01-02T00:00:00Z',
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-02T00:00:00Z',
    },
  ];

  beforeEach(() => {
    jest.clearAllMocks();
  });

  describe('initialization', () => {
    it('should load knowledge entries on mount', async () => {
      mockKnowledgeApi.getKnowledge.mockResolvedValue({ entries: mockEntries });

      const { result } = renderHook(() => useKnowledge());

      expect(result.current.loading).toBe(true);
      expect(result.current.entries).toEqual([]);

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      expect(mockKnowledgeApi.getKnowledge).toHaveBeenCalledTimes(1);
      expect(result.current.entries).toEqual(mockEntries);
      expect(result.current.error).toBeNull();
    });

    it('should handle load error', async () => {
      mockKnowledgeApi.getKnowledge.mockRejectedValue(new Error('Load failed'));

      const { result } = renderHook(() => useKnowledge());

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      expect(result.current.error).toBe('Load failed');
      expect(result.current.entries).toEqual([]);
    });

    it('should load with workspace ID', async () => {
      mockKnowledgeApi.getKnowledge.mockResolvedValue({ entries: mockEntries });

      const { result } = renderHook(() => useKnowledge('ws-1'));

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      expect(mockKnowledgeApi.getKnowledge).toHaveBeenCalledWith('ws-1');
    });
  });

  describe('createEntry', () => {
    it('should create new knowledge entry', async () => {
      const newEntry: KnowledgeEntry = {
        ...mockEntries[0],
        id: 'kb-3',
        title: 'New Entry',
      };

      mockKnowledgeApi.getKnowledge.mockResolvedValue({ entries: mockEntries });
      mockKnowledgeApi.createKnowledge.mockResolvedValue(newEntry);

      const { result } = renderHook(() => useKnowledge());

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      const request: CreateKnowledgeRequest = {
        title: 'New Entry',
        type: 'text',
        content: 'New content',
        tags: ['new'],
      };

      await act(async () => {
        await result.current.createEntry(request);
      });

      expect(mockKnowledgeApi.createKnowledge).toHaveBeenCalledWith(request);
      expect(result.current.entries).toContainEqual(newEntry);
      expect(result.current.entries).toHaveLength(mockEntries.length + 1);
    });

    it('should handle create error', async () => {
      mockKnowledgeApi.getKnowledge.mockResolvedValue({ entries: mockEntries });
      mockKnowledgeApi.createKnowledge.mockRejectedValue(new Error('Create failed'));

      const { result } = renderHook(() => useKnowledge());

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      const request: CreateKnowledgeRequest = {
        title: 'New Entry',
        type: 'text',
        content: 'New content',
      };

      await expect(result.current.createEntry(request)).rejects.toThrow('Create failed');
      expect(result.current.entries).toEqual(mockEntries);
    });
  });

  describe('deleteEntry', () => {
    it('should delete knowledge entry', async () => {
      mockKnowledgeApi.getKnowledge.mockResolvedValue({ entries: mockEntries });
      mockKnowledgeApi.deleteKnowledge.mockResolvedValue();

      const { result } = renderHook(() => useKnowledge());

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      await act(async () => {
        await result.current.deleteEntry('kb-1');
      });

      expect(mockKnowledgeApi.deleteKnowledge).toHaveBeenCalledWith('kb-1');
      expect(result.current.entries).toHaveLength(mockEntries.length - 1);
      expect(result.current.entries.find((e) => e.id === 'kb-1')).toBeUndefined();
    });

    it('should handle delete error', async () => {
      mockKnowledgeApi.getKnowledge.mockResolvedValue({ entries: mockEntries });
      mockKnowledgeApi.deleteKnowledge.mockRejectedValue(new Error('Delete failed'));

      const { result } = renderHook(() => useKnowledge());

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      await expect(result.current.deleteEntry('kb-1')).rejects.toThrow('Delete failed');
      expect(result.current.entries).toEqual(mockEntries);
    });
  });

  describe('refreshEntry', () => {
    it('should refresh knowledge entry', async () => {
      const refreshedEntry: KnowledgeEntry = {
        ...mockEntries[1],
        fetched_content: 'Updated content',
        last_refreshed_at: '2024-01-03T00:00:00Z',
      };

      mockKnowledgeApi.getKnowledge.mockResolvedValue({ entries: mockEntries });
      mockKnowledgeApi.refreshKnowledge.mockImplementation(
        () => new Promise((resolve) => setTimeout(() => resolve(refreshedEntry), 10))
      );

      const { result } = renderHook(() => useKnowledge());

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      expect(result.current.refreshing).toBeNull();

      let promise: Promise<KnowledgeEntry>;
      act(() => {
        promise = result.current.refreshEntry('kb-2');
      });

      // Check that refreshing state is set
      await waitFor(() => {
        expect(result.current.refreshing).toBe('kb-2');
      });

      await act(async () => {
        await promise;
      });

      expect(mockKnowledgeApi.refreshKnowledge).toHaveBeenCalledWith('kb-2');
      expect(result.current.refreshing).toBeNull();

      const updatedEntry = result.current.entries.find((e) => e.id === 'kb-2');
      expect(updatedEntry).toEqual(refreshedEntry);
    });

    it('should handle refresh error', async () => {
      mockKnowledgeApi.getKnowledge.mockResolvedValue({ entries: mockEntries });
      mockKnowledgeApi.refreshKnowledge.mockRejectedValue(new Error('Refresh failed'));

      const { result } = renderHook(() => useKnowledge());

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      await expect(result.current.refreshEntry('kb-2')).rejects.toThrow('Refresh failed');
      expect(result.current.refreshing).toBeNull();
      expect(result.current.entries).toEqual(mockEntries);
    });
  });

  describe('reload', () => {
    it('should reload knowledge entries', async () => {
      const updatedEntries = [
        ...mockEntries,
        {
          ...mockEntries[0],
          id: 'kb-3',
          title: 'New Entry',
        },
      ];

      mockKnowledgeApi.getKnowledge.mockResolvedValueOnce({ entries: mockEntries });

      const { result } = renderHook(() => useKnowledge());

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      expect(result.current.entries).toEqual(mockEntries);

      mockKnowledgeApi.getKnowledge.mockResolvedValueOnce({ entries: updatedEntries });

      await act(async () => {
        await result.current.reload();
      });

      expect(mockKnowledgeApi.getKnowledge).toHaveBeenCalledTimes(2);
      expect(result.current.entries).toEqual(updatedEntries);
    });

    it('should handle reload error', async () => {
      mockKnowledgeApi.getKnowledge.mockResolvedValueOnce({ entries: mockEntries });

      const { result } = renderHook(() => useKnowledge());

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      mockKnowledgeApi.getKnowledge.mockRejectedValueOnce(new Error('Reload failed'));

      await act(async () => {
        await result.current.reload();
      });

      expect(result.current.error).toBe('Reload failed');
    });
  });
});
