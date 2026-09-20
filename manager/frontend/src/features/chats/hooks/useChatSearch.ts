import { useState } from 'react';
import { chatsApi } from '../../../api/chats';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { ChatSearchResult } from '../types';

export const NO_WORKSPACE_TO_SEARCH = 'Select a workspace to search its messages.';

export function useChatSearch() {
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const [results, setResults] = useState<ChatSearchResult[]>([]);
  const [total, setTotal] = useState(0);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const search = async (query: string, options?: { chat_id?: string; limit?: number }) => {
    if (!query.trim()) {
      return;
    }
    if (!workspaceId) {
      setError(NO_WORKSPACE_TO_SEARCH);
      setResults([]);
      setTotal(0);
      return;
    }

    setSearching(true);
    setError(null);
    try {
      const response = await chatsApi.searchChatMessages({
        query: query.trim(),
        workspace_id: workspaceId,
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
