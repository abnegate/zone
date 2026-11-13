import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import type { Chat } from '../types';
import type { ChatSort } from '../utils/arrange';

const mockGetChats = mock();
const mockCreateChat = mock();
const mockDeleteChat = mock();
const mockArchiveChat = mock();
const mockUnarchiveChat = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: {
    getChats: mockGetChats,
    createChat: mockCreateChat,
    deleteChat: mockDeleteChat,
    archiveChat: mockArchiveChat,
    unarchiveChat: mockUnarchiveChat,
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

let useChats: typeof import('./useChats').useChats;
let CHAT_PAGE_SIZE: number;

beforeAll(async () => {
  ({ useChats, CHAT_PAGE_SIZE } = await import('./useChats'));
});

const page = (chats: Chat[], total = chats.length) => ({ chats, total });

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

describe('useChats', () => {
  const mockChats: Chat[] = [
    {
      id: '1',
      title: 'Chat 1',
      model_name: 'gpt-4',
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
      archived: false,
      agent_enabled: false,
    },
    {
      id: '2',
      title: 'Chat 2',
      model_name: 'gpt-3.5',
      created_at: '2024-01-02T00:00:00Z',
      updated_at: '2024-01-02T00:00:00Z',
      archived: false,
      agent_enabled: false,
    },
  ];

  beforeEach(() => {
    mockGetChats.mockReset();
    mockCreateChat.mockReset();
    mockDeleteChat.mockReset();
    mockArchiveChat.mockReset();
    mockUnarchiveChat.mockReset();
  });

  it('should fetch chats on mount', async () => {
    mockGetChats.mockResolvedValue(page(mockChats));

    const { result } = renderHook(() => useChats(), { wrapper: createWrapper() });

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.chats).toEqual(mockChats);
    expect(result.current.error).toBeNull();
    expect(mockGetChats).toHaveBeenCalledWith('test-workspace-id', {
      archived: false,
      sort: 'updated_desc',
      limit: CHAT_PAGE_SIZE,
      offset: 0,
    });
  });

  it('should fetch archived chats when archived is true', async () => {
    const archivedChats: Chat[] = [
      {
        id: '3',
        title: 'Archived Chat',
        model_name: 'gpt-4',
        created_at: '2024-01-03T00:00:00Z',
        updated_at: '2024-01-03T00:00:00Z',
        archived: true,
        agent_enabled: false,
      },
    ];
    mockGetChats.mockResolvedValue(page(archivedChats));

    const { result } = renderHook(() => useChats({ archived: true }), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.chats).toEqual(archivedChats);
    expect(mockGetChats).toHaveBeenCalledWith(
      'test-workspace-id',
      expect.objectContaining({ archived: true })
    );
  });

  it('should handle errors when fetching chats', async () => {
    const error = new Error('Failed to fetch chats');
    mockGetChats.mockRejectedValue(error);

    const { result } = renderHook(() => useChats(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.chats).toEqual([]);
    expect(result.current.error).toBe('Failed to fetch chats');
  });

  it('should create a new chat', async () => {
    const newChat: Chat = {
      id: '3',
      title: 'New Chat',
      model_name: 'gpt-4',
      created_at: '2024-01-03T00:00:00Z',
      updated_at: '2024-01-03T00:00:00Z',
      archived: false,
      agent_enabled: false,
    };
    mockGetChats.mockResolvedValue(page(mockChats));
    mockCreateChat.mockResolvedValue(newChat);

    const { result } = renderHook(() => useChats(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const createdChat = await result.current.createChat({ model_name: 'gpt-4' });

    expect(createdChat).toEqual(newChat);
    expect(mockCreateChat).toHaveBeenCalledWith({ model_name: 'gpt-4' });
    await waitFor(() => {
      expect(result.current.chats).toContainEqual(newChat);
    });
  });

  it('should delete a chat', async () => {
    mockGetChats.mockResolvedValue(page(mockChats));
    mockDeleteChat.mockResolvedValue(undefined);

    const { result } = renderHook(() => useChats(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.deleteChat('1');

    await waitFor(() => {
      expect(result.current.chats).toHaveLength(1);
    });
    expect(mockDeleteChat).toHaveBeenCalledWith('1');
    expect(result.current.chats).not.toContainEqual(mockChats[0]);
  });

  it('archiving a chat drops it from the active list at once', async () => {
    const archivedChat: Chat = { ...mockChats[0], archived: true };
    mockGetChats.mockResolvedValue(page(mockChats));
    mockArchiveChat.mockResolvedValue(archivedChat);

    const { result } = renderHook(() => useChats(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.archiveChat('1');

    await waitFor(() => {
      expect(result.current.chats.map((c) => c.id)).toEqual(['2']);
    });
    expect(mockArchiveChat).toHaveBeenCalledWith('1');
    expect(mockGetChats).toHaveBeenCalledTimes(1);
  });

  it('unarchiving a chat drops it from the archived list at once', async () => {
    const unarchivedChat: Chat = { ...mockChats[0], archived: false };
    const archivedChats: Chat[] = [
      { ...mockChats[0], archived: true },
      { ...mockChats[1], archived: true },
    ];
    mockGetChats.mockResolvedValue(page(archivedChats));
    mockUnarchiveChat.mockResolvedValue(unarchivedChat);

    const { result } = renderHook(() => useChats({ archived: true }), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.unarchiveChat('1');

    await waitFor(() => {
      expect(result.current.chats.map((c) => c.id)).toEqual(['2']);
    });
    expect(mockUnarchiveChat).toHaveBeenCalledWith('1');
  });

  it('keeps a chat listed when the server reports it still belongs to this list', async () => {
    mockGetChats.mockResolvedValue(page(mockChats));
    mockArchiveChat.mockResolvedValue({ ...mockChats[0], title: 'Renamed', archived: false });

    const { result } = renderHook(() => useChats(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.archiveChat('1');

    await waitFor(() => {
      expect(result.current.chats.find((c) => c.id === '1')?.title).toBe('Renamed');
    });
    expect(result.current.chats).toHaveLength(2);
  });

  it('should refresh chats', async () => {
    mockGetChats.mockResolvedValue(page(mockChats));

    const { result } = renderHook(() => useChats(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const updatedChats: Chat[] = [
      ...mockChats,
      {
        id: '3',
        title: 'New Chat',
        model_name: 'gpt-4',
        created_at: '2024-01-03T00:00:00Z',
        updated_at: '2024-01-03T00:00:00Z',
        archived: false,
        agent_enabled: false,
      },
    ];
    mockGetChats.mockResolvedValue(page(updatedChats));

    await result.current.refresh();

    await waitFor(() => {
      expect(result.current.chats).toEqual(updatedChats);
    });
  });

  it('keeps the listed chats when a background refresh fails', async () => {
    mockGetChats.mockResolvedValue(page(mockChats));

    const { result } = renderHook(() => useChats(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.chats).toEqual(mockChats);
    });

    mockGetChats.mockRejectedValue(new Error('Network error'));
    await act(() => result.current.refresh());

    expect(mockGetChats).toHaveBeenCalledTimes(2);
    expect(result.current.error).toBeNull();
    expect(result.current.chats).toEqual(mockChats);
  });

  describe('paging', () => {
    const numbered = (from: number, count: number): Chat[] =>
      Array.from({ length: count }, (_, index) => ({
        ...mockChats[0],
        id: `chat-${from + index}`,
        title: `Chat ${from + index}`,
      }));
    const firstPage = () => numbered(0, CHAT_PAGE_SIZE);

    it('lists one page and says how many more there are', async () => {
      mockGetChats.mockResolvedValue(page(firstPage(), CHAT_PAGE_SIZE + 3));

      const { result } = renderHook(() => useChats(), { wrapper: createWrapper() });

      await waitFor(() => expect(result.current.chats).toHaveLength(CHAT_PAGE_SIZE));
      expect(result.current.total).toBe(CHAT_PAGE_SIZE + 3);
      expect(result.current.hasMore).toBe(true);
    });

    it('appends the next page after the chats already listed, once each', async () => {
      mockGetChats.mockResolvedValueOnce(page(firstPage(), CHAT_PAGE_SIZE + 3));
      const { result } = renderHook(() => useChats({ sort: 'title_asc' }), {
        wrapper: createWrapper(),
      });
      await waitFor(() => expect(result.current.chats).toHaveLength(CHAT_PAGE_SIZE));

      const shifted = numbered(CHAT_PAGE_SIZE - 1, 4);
      mockGetChats.mockResolvedValueOnce(page(shifted, CHAT_PAGE_SIZE + 3));
      await act(() => result.current.loadMore());

      expect(mockGetChats).toHaveBeenLastCalledWith('test-workspace-id', {
        archived: false,
        sort: 'title_asc',
        limit: CHAT_PAGE_SIZE,
        offset: CHAT_PAGE_SIZE,
      });
      expect(result.current.chats.map((chat) => chat.id)).toEqual(
        numbered(0, CHAT_PAGE_SIZE + 3).map((chat) => chat.id)
      );
      expect(result.current.hasMore).toBe(false);
      expect(result.current.loadingMore).toBe(false);
    });

    it('asks for nothing more once every chat is listed', async () => {
      mockGetChats.mockResolvedValue(page(mockChats));
      const { result } = renderHook(() => useChats(), { wrapper: createWrapper() });
      await waitFor(() => expect(result.current.chats).toEqual(mockChats));

      await act(() => result.current.loadMore());

      expect(mockGetChats).toHaveBeenCalledTimes(1);
    });

    it('reorders from the server without hiding the list when the sort changes', async () => {
      mockGetChats.mockResolvedValueOnce(page(firstPage(), CHAT_PAGE_SIZE + 3));
      mockGetChats.mockResolvedValueOnce(page(numbered(CHAT_PAGE_SIZE, 3), CHAT_PAGE_SIZE + 3));
      const { result, rerender } = renderHook(({ sort }) => useChats({ sort }), {
        initialProps: { sort: 'updated_desc' as ChatSort },
        wrapper: createWrapper(),
      });
      await waitFor(() => expect(result.current.chats).toHaveLength(CHAT_PAGE_SIZE));
      await act(() => result.current.loadMore());

      let resolve: (value: unknown) => void = () => {};
      mockGetChats.mockImplementationOnce(
        () =>
          new Promise((done) => {
            resolve = done;
          })
      );
      rerender({ sort: 'title_desc' });

      expect(result.current.loading).toBe(false);
      expect(result.current.chats).toHaveLength(CHAT_PAGE_SIZE + 3);
      expect(mockGetChats).toHaveBeenLastCalledWith('test-workspace-id', {
        archived: false,
        sort: 'title_desc',
        limit: CHAT_PAGE_SIZE + 3,
        offset: 0,
      });
      const reordered = numbered(0, CHAT_PAGE_SIZE + 3).reverse();
      await act(async () => resolve(page(reordered)));
      expect(result.current.chats[0]?.id).toBe(reordered[0]?.id);
    });

    it('shows loading again when the list changes to the archived chats', async () => {
      mockGetChats.mockResolvedValue(page(mockChats));
      const { result, rerender } = renderHook(({ archived }) => useChats({ archived }), {
        initialProps: { archived: false },
        wrapper: createWrapper(),
      });
      await waitFor(() => expect(result.current.chats).toEqual(mockChats));

      mockGetChats.mockImplementationOnce(() => new Promise(() => {}));
      rerender({ archived: true });

      await waitFor(() => expect(result.current.loading).toBe(true));
    });

    it('counts a deleted chat out of the total', async () => {
      mockGetChats.mockResolvedValue(page(mockChats, 30));
      mockDeleteChat.mockResolvedValue(undefined);
      const { result } = renderHook(() => useChats(), { wrapper: createWrapper() });
      await waitFor(() => expect(result.current.total).toBe(30));

      await act(() => result.current.deleteChat('1'));

      expect(result.current.total).toBe(29);
    });

    it('counts a created chat into the total', async () => {
      mockGetChats.mockResolvedValue(page(mockChats, 30));
      mockCreateChat.mockResolvedValue({ ...mockChats[0], id: 'new' });
      const { result } = renderHook(() => useChats(), { wrapper: createWrapper() });
      await waitFor(() => expect(result.current.total).toBe(30));

      await act(async () => {
        await result.current.createChat({
          workspace_id: 'test-workspace-id',
          title: 'New',
          model_name: 'gpt-4',
        });
      });

      expect(result.current.total).toBe(31);
    });
  });
});
