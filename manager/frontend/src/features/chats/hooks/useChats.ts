import { useCallback, useEffect, useRef, useState } from 'react';
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
  const titles = useRef(new Map<string, { title: string; revision: number }>());
  const revision = useRef(0);
  const request = useRef(0);

  const updateTitle = useCallback((id: string, title: string): void => {
    titles.current.set(id, { title, revision: ++revision.current });
    setChats((previous) => previous.map((chat) => (chat.id === id ? { ...chat, title } : chat)));
  }, []);

  const renameChat = async (id: string, title: string): Promise<Chat> => {
    const updated = await chatsApi.updateChat(id, { title });
    updateTitle(id, updated.title);
    return updated;
  };

  const fetchChats = useCallback(
    async (opts?: { silent?: boolean }) => {
      const currentRequest = ++request.current;
      const currentRevision = revision.current;
      if (!workspaceId) {
        setChats([]);
        setLoading(false);
        return;
      }
      if (!opts?.silent) {
        setLoading(true);
      }
      setError(null);
      try {
        const data = await chatsApi.getChats(workspaceId, archived);
        if (currentRequest !== request.current) return;
        setChats(
          data.map((chat) => {
            const updated = titles.current.get(chat.id);
            return updated && updated.revision > currentRevision
              ? { ...chat, title: updated.title }
              : chat;
          })
        );
      } catch (err) {
        if (currentRequest !== request.current) return;
        setError(err instanceof Error ? err.message : 'Failed to fetch chats');
      } finally {
        if (currentRequest === request.current) setLoading(false);
      }
    },
    [workspaceId, archived]
  );

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
    await fetchChats({ silent: true });
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
    renameChat,
    updateTitle,
  };
}
