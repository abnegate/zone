import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type { ChatSource } from '../types';

/**
 * The sources a chat's retrieval is confined to. Loaded per chat and kept
 * per chat: switching chats never carries one chat's attachment to another.
 */
export function useChatSources(chatId: string | null) {
  const [sources, setSources] = useState<ChatSource[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);

  useEffect(() => {
    const current = ++request.current;
    setError(null);
    if (!chatId) {
      setSources([]);
      setLoading(false);
      return;
    }
    setLoading(true);
    chatsApi
      .getChatSources(chatId)
      .then((attached) => {
        if (current !== request.current) return;
        setSources(attached);
      })
      .catch((err: unknown) => {
        if (current !== request.current) return;
        setSources([]);
        setError(err instanceof Error ? err.message : 'Failed to load attached sources');
      })
      .finally(() => {
        if (current === request.current) setLoading(false);
      });
  }, [chatId]);

  const setAttached = useCallback(
    async (sourceIds: string[]): Promise<void> => {
      if (!chatId) return;
      const current = request.current;
      setError(null);
      try {
        const attached = await chatsApi.setChatSources(chatId, sourceIds);
        if (current !== request.current) return;
        setSources(attached);
      } catch (err) {
        if (current !== request.current) return;
        setError(err instanceof Error ? err.message : 'Failed to update attached sources');
      }
    },
    [chatId]
  );

  return { sources, loading, error, setAttached };
}
