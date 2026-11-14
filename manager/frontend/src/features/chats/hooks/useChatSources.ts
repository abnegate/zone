import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type { ChatSource } from '../types';
import { cached } from '../utils/cached';

interface Attachment {
  chatId: string | null;
  sources: ChatSource[];
}

const NOTHING_ATTACHED: Attachment = { chatId: null, sources: [] };

/**
 * The sources a chat's retrieval is confined to. Loaded per chat and kept
 * per chat: switching chats never carries one chat's attachment to another.
 */
export function useChatSources(chatId: string | null) {
  const [attachment, setAttachment] = useState<Attachment>(NOTHING_ATTACHED);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);
  const remembered = chatId ? cached.sources.peek(chatId) : undefined;
  const sources =
    attachment.chatId === chatId ? attachment.sources : (remembered ?? NOTHING_ATTACHED.sources);

  const keep = useCallback((id: string, attached: ChatSource[]) => {
    cached.sources.remember(id, attached);
    setAttachment({ chatId: id, sources: attached });
  }, []);

  useEffect(() => {
    const current = ++request.current;
    setError(null);
    if (!chatId) {
      setAttachment(NOTHING_ATTACHED);
      setLoading(false);
      return;
    }
    setLoading(!cached.sources.has(chatId));
    chatsApi
      .getChatSources(chatId)
      .then((attached) => {
        if (current !== request.current) return;
        keep(chatId, attached);
      })
      .catch((err: unknown) => {
        if (current !== request.current) return;
        cached.sources.forget(chatId);
        setAttachment({ chatId, sources: [] });
        setError(err instanceof Error ? err.message : 'Failed to load attached sources');
      })
      .finally(() => {
        if (current === request.current) setLoading(false);
      });
  }, [chatId, keep]);

  const setAttached = useCallback(
    async (sourceIds: string[]): Promise<void> => {
      if (!chatId) return;
      const current = request.current;
      setError(null);
      try {
        const attached = await chatsApi.setChatSources(chatId, sourceIds);
        if (current !== request.current) return;
        keep(chatId, attached);
      } catch (err) {
        if (current !== request.current) return;
        setError(err instanceof Error ? err.message : 'Failed to update attached sources');
      }
    },
    [chatId, keep]
  );

  return { sources, loading: loading && remembered === undefined, error, setAttached };
}
