import { renderHook, waitFor, act } from '@testing-library/react';
import { useChatSearch } from './useChatSearch';
import { chatsApi } from '../../../api/chats';
import type { ChatSearchResult } from '../types';

jest.mock('../../../api/chats');

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
    jest.clearAllMocks();
  });

  it('should not search on mount', () => {
    (chatsApi.searchChatMessages as jest.Mock).mockResolvedValue({
      results: mockSearchResults,
      total: 2,
    });

    const { result } = renderHook(() => useChatSearch());

    expect(result.current.results).toEqual([]);
    expect(result.current.searching).toBe(false);
    expect(result.current.error).toBeNull();
    expect(chatsApi.searchChatMessages).not.toHaveBeenCalled();
  });

  it('should search when search is called', async () => {
    (chatsApi.searchChatMessages as jest.Mock).mockResolvedValue({
      results: mockSearchResults,
      total: 2,
    });

    const { result } = renderHook(() => useChatSearch());

    act(() => {
      result.current.search('hello');
    });

    await waitFor(() => {
      expect(result.current.searching).toBe(false);
    });

    expect(result.current.results).toEqual(mockSearchResults);
    expect(result.current.total).toBe(2);
    expect(result.current.error).toBeNull();
    expect(chatsApi.searchChatMessages).toHaveBeenCalledWith({ query: 'hello' });
  });

  it('should search with chat_id filter', async () => {
    (chatsApi.searchChatMessages as jest.Mock).mockResolvedValue({
      results: [mockSearchResults[0]],
      total: 1,
    });

    const { result } = renderHook(() => useChatSearch());

    act(() => {
      result.current.search('hello', { chat_id: 'c1' });
    });

    await waitFor(() => {
      expect(result.current.searching).toBe(false);
    });

    expect(result.current.results).toEqual([mockSearchResults[0]]);
    expect(result.current.total).toBe(1);
    expect(chatsApi.searchChatMessages).toHaveBeenCalledWith({ query: 'hello', chat_id: 'c1' });
  });

  it('should search with limit', async () => {
    (chatsApi.searchChatMessages as jest.Mock).mockResolvedValue({
      results: [mockSearchResults[0]],
      total: 2,
    });

    const { result } = renderHook(() => useChatSearch());

    act(() => {
      result.current.search('hello', { limit: 1 });
    });

    await waitFor(() => {
      expect(result.current.searching).toBe(false);
    });

    expect(result.current.results).toEqual([mockSearchResults[0]]);
    expect(result.current.total).toBe(2);
    expect(chatsApi.searchChatMessages).toHaveBeenCalledWith({ query: 'hello', limit: 1 });
  });

  it('should handle errors when searching', async () => {
    const error = new Error('Search failed');
    (chatsApi.searchChatMessages as jest.Mock).mockRejectedValue(error);

    const { result } = renderHook(() => useChatSearch());

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
    (chatsApi.searchChatMessages as jest.Mock).mockResolvedValue({
      results: mockSearchResults,
      total: 2,
    });

    const { result } = renderHook(() => useChatSearch());

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
    (chatsApi.searchChatMessages as jest.Mock).mockResolvedValue({
      results: mockSearchResults,
      total: 2,
    });

    const { result } = renderHook(() => useChatSearch());

    act(() => {
      result.current.search('');
    });

    expect(result.current.searching).toBe(false);
    expect(chatsApi.searchChatMessages).not.toHaveBeenCalled();
  });

  it('should handle multiple searches in sequence', async () => {
    (chatsApi.searchChatMessages as jest.Mock)
      .mockResolvedValueOnce({
        results: [mockSearchResults[0]],
        total: 1,
      })
      .mockResolvedValueOnce({
        results: [mockSearchResults[1]],
        total: 1,
      });

    const { result } = renderHook(() => useChatSearch());

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
