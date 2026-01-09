import { useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type { ChatSearchResult } from '../types';

export function useChatSearch() {
  const [results, setResults] = useState<ChatSearchResult[]>([]);
  const [total, setTotal] = useState(0);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const search = async (query: string, options?: { chat_id?: string; limit?: number }) => {
    if (!query.trim()) {
      return;
    }

    setSearching(true);
    setError(null);
    try {
      const response = await chatsApi.searchChatMessages({
        query: query.trim(),
        ...options,
      });
      setResults(response.results);
      setTotal(response.total);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Search failed');
      setResults([]);
      setTotal(0);
    } finally {
      setSearching(false);
    }
  };

  const clear = () => {
    setResults([]);
    setTotal(0);
    setError(null);
  };

  return {
    results,
    total,
    searching,
    error,
    search,
    clear,
  };
}
