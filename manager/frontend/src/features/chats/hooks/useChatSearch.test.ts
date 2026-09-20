import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import type { ChatSearchResult } from '../types';

const mockSearchChatMessages = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: {
    searchChatMessages: mockSearchChatMessages,
  },
}));

let workspace: { id: string; name: string } | null = { id: 'ws-1', name: 'Test Workspace' };

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentWorkspace: workspace,
    currentOrganization: { id: 'org-1', name: 'Test Org' },
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

let useChatSearch: typeof import('./useChatSearch').useChatSearch;
let NO_WORKSPACE_TO_SEARCH: string;

beforeAll(async () => {
  ({ useChatSearch, NO_WORKSPACE_TO_SEARCH } = await import('./useChatSearch'));
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

describe('useChatSearch', () => {
  const mockSearchResults: ChatSearchResult[] = [
    {
      message_id: 'm1',
      chat_id: 'c1',
      chat_title: 'Test Chat',
      content: 'Hello world',
      snippet: 'Hello world',
      relevance_score: 0.95,
      created_at: '2024-01-01T00:00:00Z',
    },
    {
      message_id: 'm2',
      chat_id: 'c2',
      chat_title: 'Another Chat',
      content: 'Hello again',
      snippet: 'Hello again',
      relevance_score: 0.85,
      created_at: '2024-01-02T00:00:00Z',
    },
  ];

  beforeEach(() => {
    mockSearchChatMessages.mockReset();
    workspace = { id: 'ws-1', name: 'Test Workspace' };
  });

  it('should not search on mount', () => {
    mockSearchChatMessages.mockResolvedValue({
      results: mockSearchResults,
      total: 2,
    });

    const { result } = renderHook(() => useChatSearch(), { wrapper: createWrapper() });

    expect(result.current.results).toEqual([]);
    expect(result.current.searching).toBe(false);
    expect(result.current.error).toBeNull();
    expect(mockSearchChatMessages).not.toHaveBeenCalled();
  });

  it('should search when search is called', async () => {
    mockSearchChatMessages.mockResolvedValue({
      results: mockSearchResults,
      total: 2,
    });

    const { result } = renderHook(() => useChatSearch(), { wrapper: createWrapper() });

    act(() => {
      result.current.search('hello');
    });

    await waitFor(() => {
      expect(result.current.searching).toBe(false);
    });

    expect(result.current.results).toEqual(mockSearchResults);
    expect(result.current.total).toBe(2);
    expect(result.current.error).toBeNull();
    expect(mockSearchChatMessages).toHaveBeenCalledWith({ query: 'hello', workspace_id: 'ws-1' });
  });

  it('should search with chat_id filter', async () => {
    mockSearchChatMessages.mockResolvedValue({
      results: [mockSearchResults[0]],
      total: 1,
    });

    const { result } = renderHook(() => useChatSearch(), { wrapper: createWrapper() });

    act(() => {
      result.current.search('hello', { chat_id: 'c1' });
    });

    await waitFor(() => {
      expect(result.current.searching).toBe(false);
    });

    expect(result.current.results).toEqual([mockSearchResults[0]]);
    expect(result.current.total).toBe(1);
    expect(mockSearchChatMessages).toHaveBeenCalledWith({
      query: 'hello',
      workspace_id: 'ws-1',
      chat_id: 'c1',
    });
  });

  it('should search with limit', async () => {
    mockSearchChatMessages.mockResolvedValue({
      results: [mockSearchResults[0]],
      total: 2,
    });

    const { result } = renderHook(() => useChatSearch(), { wrapper: createWrapper() });

    act(() => {
      result.current.search('hello', { limit: 1 });
    });

    await waitFor(() => {
      expect(result.current.searching).toBe(false);
    });

    expect(result.current.results).toEqual([mockSearchResults[0]]);
    expect(result.current.total).toBe(2);
    expect(mockSearchChatMessages).toHaveBeenCalledWith({
      query: 'hello',
      workspace_id: 'ws-1',
      limit: 1,
    });
  });

  it('should handle errors when searching', async () => {
    const error = new Error('Search failed');
    mockSearchChatMessages.mockRejectedValue(error);

    const { result } = renderHook(() => useChatSearch(), { wrapper: createWrapper() });

    act(() => {
      result.current.search('hello');
    });

    await waitFor(() => {
      expect(result.current.searching).toBe(false);
    });

    expect(result.current.results).toEqual([]);
    expect(result.current.total).toBe(0);
    expect(result.current.error).toBe('Search failed');
  });

  it('should clear search results', async () => {
    mockSearchChatMessages.mockResolvedValue({
      results: mockSearchResults,
      total: 2,
    });

    const { result } = renderHook(() => useChatSearch(), { wrapper: createWrapper() });

    act(() => {
      result.current.search('hello');
    });

    await waitFor(() => {
      expect(result.current.searching).toBe(false);
    });

    expect(result.current.results).toEqual(mockSearchResults);

    act(() => {
      result.current.clear();
    });

    expect(result.current.results).toEqual([]);
    expect(result.current.total).toBe(0);
    expect(result.current.error).toBeNull();
  });

  it('should not search with empty query', async () => {
    mockSearchChatMessages.mockResolvedValue({
      results: mockSearchResults,
      total: 2,
    });

    const { result } = renderHook(() => useChatSearch(), { wrapper: createWrapper() });

    act(() => {
      result.current.search('');
    });

    expect(result.current.searching).toBe(false);
    expect(mockSearchChatMessages).not.toHaveBeenCalled();
  });

  it('surfaces the server failure text instead of an empty result list', async () => {
    mockSearchChatMessages.mockRejectedValue(
      new Error('Failed to search chat messages: missing field workspace_id')
    );

    const { result } = renderHook(() => useChatSearch(), { wrapper: createWrapper() });

    act(() => {
      result.current.search('zebra');
    });

    await waitFor(() => {
      expect(result.current.searching).toBe(false);
    });

    expect(result.current.error).toBe('Failed to search chat messages: missing field workspace_id');
    expect(result.current.results).toEqual([]);
  });

  it('refuses to search without a workspace and says so', async () => {
    workspace = null;

    const { result } = renderHook(() => useChatSearch(), { wrapper: createWrapper() });

    await act(async () => {
      await result.current.search('hello');
    });

    expect(mockSearchChatMessages).not.toHaveBeenCalled();
    expect(result.current.error).toBe(NO_WORKSPACE_TO_SEARCH);
  });

  it('should handle multiple searches in sequence', async () => {
    mockSearchChatMessages
      .mockResolvedValueOnce({
        results: [mockSearchResults[0]],
        total: 1,
      })
      .mockResolvedValueOnce({
        results: [mockSearchResults[1]],
        total: 1,
      });

    const { result } = renderHook(() => useChatSearch(), { wrapper: createWrapper() });

    act(() => {
      result.current.search('first');
    });
    await waitFor(() => expect(result.current.searching).toBe(false));
    expect(result.current.results).toEqual([mockSearchResults[0]]);

    act(() => {
      result.current.search('second');
    });
    await waitFor(() => expect(result.current.searching).toBe(false));
    expect(result.current.results).toEqual([mockSearchResults[1]]);
  });
});
