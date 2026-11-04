import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type { AiProvider } from '../../settings/workspace/types';
import type { ChatProviders, ChatWithMessages } from '../types';

const LOAD_FAILED = 'Failed to load AI providers';

/**
 * The providers one chat may run on, its own choice among them and the
 * workspace default it otherwise follows. A refetch that fails keeps this
 * chat's last list and reports the error until a later one succeeds, so the
 * list reads as stale throughout; a load that lands after the chat changed
 * never shows another chat's list.
 */
export function useChatProviders(chatId: string | null) {
  const [providers, setProviders] = useState<ChatProviders | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);
  const latestChatId = useRef(chatId);

  const load = useCallback(async (id: string | null): Promise<void> => {
    const current = ++request.current;
    if (!id) {
      setProviders(null);
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const loaded = await chatsApi.getChatProviders(id);
      if (current === request.current) {
        setProviders(loaded);
        setError(null);
      }
    } catch (error) {
      if (current !== request.current) return;
      setError(error instanceof Error ? error.message : LOAD_FAILED);
    } finally {
      if (current === request.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    latestChatId.current = chatId;
    setProviders(null);
    setError(null);
    void load(chatId);
  }, [chatId, load]);

  const refresh = useCallback((): Promise<void> => load(latestChatId.current), [load]);

  const choose = useCallback(
    async (provider: AiProvider | null, model: string | null = null): Promise<ChatWithMessages> => {
      if (!chatId) throw new Error('No chat selected');
      const chat = await chatsApi.setChatProvider(chatId, { provider, model });
      if (latestChatId.current === chatId) await load(chatId);
      return chat;
    },
    [chatId, load]
  );

  return { providers, loading, error, choose, refresh };
}
