import { act, renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import type { KnowledgeEntry, CreateKnowledgeRequest } from '../types';
import { createElement } from 'react';
import type { ReactNode } from 'react';

const mockGetKnowledge = mock();
const mockCreateKnowledge = mock();
const mockDeleteKnowledge = mock();
const mockRefreshKnowledge = mock();

mock.module('../../../api/knowledge', () => ({
  knowledgeApi: {
    getKnowledge: mockGetKnowledge,
    createKnowledge: mockCreateKnowledge,
    deleteKnowledge: mockDeleteKnowledge,
    refreshKnowledge: mockRefreshKnowledge,
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

let useKnowledge: typeof import('./useKnowledge').useKnowledge;

beforeAll(async () => {
  ({ useKnowledge } = await import('./useKnowledge'));
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
    mockGetKnowledge.mockReset();
    mockCreateKnowledge.mockReset();
    mockDeleteKnowledge.mockReset();
    mockRefreshKnowledge.mockReset();
  });

  describe('initialization', () => {
    it('should load knowledge entries on mount', async () => {
      mockGetKnowledge.mockResolvedValue({ entries: mockEntries });

      const { result } = renderHook(() => useKnowledge(), { wrapper: createWrapper() });

      expect(result.current.loading).toBe(true);
      expect(result.current.entries).toEqual([]);

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      expect(mockGetKnowledge).toHaveBeenCalledTimes(1);
      expect(result.current.entries).toEqual(mockEntries);
      expect(result.current.error).toBeNull();
    });

    it('should handle load error', async () => {
      mockGetKnowledge.mockRejectedValue(new Error('Load failed'));

      const { result } = renderHook(() => useKnowledge(), { wrapper: createWrapper() });

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      expect(result.current.error).toBe('Load failed');
      expect(result.current.entries).toEqual([]);
    });

    it('should load with workspace ID', async () => {
      mockGetKnowledge.mockResolvedValue({ entries: mockEntries });

      const { result } = renderHook(() => useKnowledge(), { wrapper: createWrapper() });

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      expect(mockGetKnowledge).toHaveBeenCalledWith('test-workspace-id');
    });
  });

  describe('createEntry', () => {
    it('should create new knowledge entry', async () => {
      const newEntry: KnowledgeEntry = {
        ...mockEntries[0],
        id: 'kb-3',
        title: 'New Entry',
      };

      mockGetKnowledge.mockResolvedValue({ entries: mockEntries });
      mockCreateKnowledge.mockResolvedValue(newEntry);

      const { result } = renderHook(() => useKnowledge(), { wrapper: createWrapper() });

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

      // Hook adds workspace_id to the request
      expect(mockCreateKnowledge).toHaveBeenCalledWith({
        ...request,
        workspace_id: 'test-workspace-id',
      });
      expect(result.current.entries).toContainEqual(newEntry);
      expect(result.current.entries).toHaveLength(mockEntries.length + 1);
    });

    it('should handle create error', async () => {
      mockGetKnowledge.mockResolvedValue({ entries: mockEntries });
      mockCreateKnowledge.mockRejectedValue(new Error('Create failed'));

      const { result } = renderHook(() => useKnowledge(), { wrapper: createWrapper() });

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
      mockGetKnowledge.mockResolvedValue({ entries: mockEntries });
      mockDeleteKnowledge.mockResolvedValue();

      const { result } = renderHook(() => useKnowledge(), { wrapper: createWrapper() });

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      await act(async () => {
        await result.current.deleteEntry('kb-1');
      });

      expect(mockDeleteKnowledge).toHaveBeenCalledWith('kb-1');
      expect(result.current.entries).toHaveLength(mockEntries.length - 1);
      expect(result.current.entries.find((e) => e.id === 'kb-1')).toBeUndefined();
    });

    it('should handle delete error', async () => {
      mockGetKnowledge.mockResolvedValue({ entries: mockEntries });
      mockDeleteKnowledge.mockRejectedValue(new Error('Delete failed'));

      const { result } = renderHook(() => useKnowledge(), { wrapper: createWrapper() });

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

      mockGetKnowledge.mockResolvedValue({ entries: mockEntries });
      mockRefreshKnowledge.mockImplementation(
        () => new Promise((resolve) => setTimeout(() => resolve(refreshedEntry), 10))
      );

      const { result } = renderHook(() => useKnowledge(), { wrapper: createWrapper() });

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

      expect(mockRefreshKnowledge).toHaveBeenCalledWith('kb-2');
      expect(result.current.refreshing).toBeNull();

      const updatedEntry = result.current.entries.find((e) => e.id === 'kb-2');
      expect(updatedEntry).toEqual(refreshedEntry);
    });

    it('should handle refresh error', async () => {
      mockGetKnowledge.mockResolvedValue({ entries: mockEntries });
      mockRefreshKnowledge.mockRejectedValue(new Error('Refresh failed'));

      const { result } = renderHook(() => useKnowledge(), { wrapper: createWrapper() });

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

      mockGetKnowledge.mockResolvedValueOnce({ entries: mockEntries });

      const { result } = renderHook(() => useKnowledge(), { wrapper: createWrapper() });

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      expect(result.current.entries).toEqual(mockEntries);

      mockGetKnowledge.mockResolvedValueOnce({ entries: updatedEntries });

      await act(async () => {
        await result.current.reload();
      });

      expect(mockGetKnowledge).toHaveBeenCalledTimes(2);
      expect(result.current.entries).toEqual(updatedEntries);
    });

    it('should handle reload error', async () => {
      mockGetKnowledge.mockResolvedValueOnce({ entries: mockEntries });

      const { result } = renderHook(() => useKnowledge(), { wrapper: createWrapper() });

      await waitFor(() => {
        expect(result.current.loading).toBe(false);
      });

      mockGetKnowledge.mockRejectedValueOnce(new Error('Reload failed'));

      await act(async () => {
        await result.current.reload();
      });

      expect(result.current.error).toBe('Reload failed');
    });
  });
});
