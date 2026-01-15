import { useState, useEffect, useCallback } from 'react';
import { chatsApi } from '../../../api/chats';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { Chat, CreateChatRequest } from '../types';

export interface UseChatsOptions {
  archived?: boolean;
}

export function useChats(options: UseChatsOptions = {}) {
  const { archived = false } = options;
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const [chats, setChats] = useState<Chat[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchChats = useCallback(async () => {
    if (!workspaceId) {
      setChats([]);
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const data = await chatsApi.getChats(workspaceId, archived);
      setChats(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to fetch chats');
    } finally {
      setLoading(false);
    }
  }, [workspaceId, archived]);

  useEffect(() => {
    fetchChats();
  }, [fetchChats]);

  const createChat = async (request: CreateChatRequest): Promise<Chat> => {
    const chat = await chatsApi.createChat(request);
    setChats((prev) => [chat, ...prev]);
    return chat;
  };

  const deleteChat = async (id: string): Promise<void> => {
    await chatsApi.deleteChat(id);
    setChats((prev) => prev.filter((c) => c.id !== id));
  };

  const archiveChat = async (id: string): Promise<void> => {
    const archivedChat = await chatsApi.archiveChat(id);
    setChats((prev) => prev.map((c) => (c.id === id ? archivedChat : c)));
  };

  const unarchiveChat = async (id: string): Promise<void> => {
    const unarchivedChat = await chatsApi.unarchiveChat(id);
    setChats((prev) => prev.map((c) => (c.id === id ? unarchivedChat : c)));
  };

  const refresh = async (): Promise<void> => {
    await fetchChats();
  };

  return {
    chats,
    loading,
    error,
    createChat,
    deleteChat,
    archiveChat,
    unarchiveChat,
    refresh,
  };
}
