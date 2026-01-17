import { renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import type { Chat } from '../types';
import { createElement } from 'react';
import type { ReactNode } from 'react';

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

beforeAll(async () => {
  ({ useChats } = await import('./useChats'));
});

afterAll(() => {
  mock.restore();
});

const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
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
    },
    {
      id: '2',
      title: 'Chat 2',
      model_name: 'gpt-3.5',
      created_at: '2024-01-02T00:00:00Z',
      updated_at: '2024-01-02T00:00:00Z',
      archived: false,
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
    mockGetChats.mockResolvedValue(mockChats);

    const { result } = renderHook(() => useChats(), { wrapper: createWrapper() });

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.chats).toEqual(mockChats);
    expect(result.current.error).toBeNull();
    expect(mockGetChats).toHaveBeenCalledWith('test-workspace-id', false);
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
      },
    ];
    mockGetChats.mockResolvedValue(archivedChats);

    const { result } = renderHook(() => useChats({ archived: true }), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.chats).toEqual(archivedChats);
    expect(mockGetChats).toHaveBeenCalledWith('test-workspace-id', true);
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
    };
    mockGetChats.mockResolvedValue(mockChats);
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
    mockGetChats.mockResolvedValue(mockChats);
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

  it('should archive a chat', async () => {
    const archivedChat: Chat = { ...mockChats[0], archived: true };
    mockGetChats.mockResolvedValue(mockChats);
    mockArchiveChat.mockResolvedValue(archivedChat);

    const { result } = renderHook(() => useChats(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.archiveChat('1');

    await waitFor(() => {
      expect(result.current.chats.find((c) => c.id === '1')?.archived).toBe(true);
    });
    expect(mockArchiveChat).toHaveBeenCalledWith('1');
  });

  it('should unarchive a chat', async () => {
    const unarchivedChat: Chat = { ...mockChats[0], archived: false };
    const archivedChats: Chat[] = [{ ...mockChats[0], archived: true }];
    mockGetChats.mockResolvedValue(archivedChats);
    mockUnarchiveChat.mockResolvedValue(unarchivedChat);

    const { result } = renderHook(() => useChats({ archived: true }), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.unarchiveChat('1');

    await waitFor(() => {
      expect(result.current.chats.find((c) => c.id === '1')?.archived).toBe(false);
    });
    expect(mockUnarchiveChat).toHaveBeenCalledWith('1');
  });

  it('should refresh chats', async () => {
    mockGetChats.mockResolvedValue(mockChats);

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
      },
    ];
    mockGetChats.mockResolvedValue(updatedChats);

    await result.current.refresh();

    await waitFor(() => {
      expect(result.current.chats).toEqual(updatedChats);
    });
  });
});
