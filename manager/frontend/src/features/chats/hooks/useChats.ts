import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { Chat, CreateChatRequest } from '../types';
import type { ChatSort } from '../utils/arrange';

export const CHAT_PAGE_SIZE = 25;

export interface UseChatsOptions {
  archived?: boolean;
  sort?: ChatSort;
}

function append(existing: readonly Chat[], page: readonly Chat[]): Chat[] {
  const listed = new Set(existing.map((chat) => chat.id));
  return [...existing, ...page.filter((chat) => !listed.has(chat.id))];
}

export function useChats(options: UseChatsOptions = {}) {
  const { archived = false, sort = 'updated_desc' } = options;
  const { currentWorkspace, loading: workspaceLoading } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const [chats, setChats] = useState<Chat[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const titles = useRef(new Map<string, { title: string; revision: number }>());
  const revision = useRef(0);
  const request = useRef(0);
  const scope = `${workspaceId}:${archived}`;
  const listedScope = useRef<string | null>(null);
  const listed = useRef(0);
  listed.current = chats.length;

  const updateTitle = useCallback((id: string, title: string): void => {
    titles.current.set(id, { title, revision: ++revision.current });
    setChats((previous) => previous.map((chat) => (chat.id === id ? { ...chat, title } : chat)));
  }, []);

  const renameChat = async (id: string, title: string): Promise<Chat> => {
    const updated = await chatsApi.updateChat(id, { title });
    updateTitle(id, updated.title);
    return updated;
  };

  const retitled = useCallback(
    (page: readonly Chat[], since: number): Chat[] =>
      page.map((chat) => {
        const updated = titles.current.get(chat.id);
        return updated && updated.revision > since ? { ...chat, title: updated.title } : chat;
      }),
    []
  );

  const fetchChats = useCallback(
    async ({ silent = false }: { silent?: boolean } = {}) => {
      const currentRequest = ++request.current;
      const currentRevision = revision.current;
      if (!workspaceId) {
        setChats([]);
        setTotal(0);
        setLoading(false);
        return;
      }
      if (!silent) {
        setLoading(true);
      }
      setLoadingMore(false);
      setError(null);
      try {
        const page = await chatsApi.getChats(workspaceId, {
          archived,
          sort,
          limit: silent ? Math.max(listed.current, CHAT_PAGE_SIZE) : CHAT_PAGE_SIZE,
          offset: 0,
        });
        if (currentRequest !== request.current) return;
        setChats(retitled(page.chats, currentRevision));
        setTotal(page.total);
        listedScope.current = scope;
      } catch (error) {
        if (currentRequest !== request.current || silent) return;
        setError(error instanceof Error ? error.message : 'Failed to fetch chats');
      } finally {
        if (currentRequest === request.current) setLoading(false);
      }
    },
    [workspaceId, archived, sort, scope, retitled]
  );

  useEffect(() => {
    fetchChats({ silent: listedScope.current === scope });
  }, [fetchChats, scope]);

  const hasMore = chats.length < total;

  const loadMore = useCallback(async (): Promise<void> => {
    if (!workspaceId || loading || loadingMore || !hasMore) return;
    const currentRequest = request.current;
    const currentRevision = revision.current;
    setLoadingMore(true);
    try {
      const page = await chatsApi.getChats(workspaceId, {
        archived,
        sort,
        limit: CHAT_PAGE_SIZE,
        offset: listed.current,
      });
      if (currentRequest !== request.current) return;
      setChats((previous) => append(previous, retitled(page.chats, currentRevision)));
      setTotal(page.total);
    } catch (error) {
      if (currentRequest !== request.current) return;
      setError(error instanceof Error ? error.message : 'Failed to fetch chats');
    } finally {
      if (currentRequest === request.current) setLoadingMore(false);
    }
  }, [workspaceId, archived, sort, loading, loadingMore, hasMore, retitled]);

  const createChat = async (request: CreateChatRequest): Promise<Chat> => {
    const chat = await chatsApi.createChat(request);
    setChats((prev) => [chat, ...prev]);
    setTotal((count) => count + 1);
    return chat;
  };

  const remove = (id: string): void => {
    if (chats.some((chat) => chat.id === id)) setTotal((count) => Math.max(0, count - 1));
    setChats((prev) => prev.filter((chat) => chat.id !== id));
  };

  const deleteChat = async (id: string): Promise<void> => {
    await chatsApi.deleteChat(id);
    remove(id);
  };

  const place = (updated: Chat): void => {
    if (updated.archived !== archived) {
      remove(updated.id);
      return;
    }
    setChats((prev) => prev.map((c) => (c.id === updated.id ? updated : c)));
  };

  const archiveChat = async (id: string): Promise<void> => {
    place(await chatsApi.archiveChat(id));
  };

  const unarchiveChat = async (id: string): Promise<void> => {
    place(await chatsApi.unarchiveChat(id));
  };

  const refresh = async (): Promise<void> => {
    await fetchChats({ silent: true });
  };

  return {
    chats,
    total,
    hasMore,
    loading: workspaceId ? loading : workspaceLoading,
    loadingMore,
    error,
    loadMore,
    createChat,
    deleteChat,
    archiveChat,
    unarchiveChat,
    refresh,
    renameChat,
    updateTitle,
  };
}
