import { useEffect, useRef } from 'react';
import { chatsApi } from '../../../api/chats';
import type { Chat } from '../types';
import { histories } from '../utils/history';

export const PRELOADS_AT_ONCE = 3;

/**
 * Loads the history of every listed chat into the history cache, a few at a
 * time, so opening any chat on a loaded page draws it at once.
 */
export function usePreloadChats(chats: readonly Chat[]): void {
  const attempted = useRef(new Set<string>());
  const queue = useRef<string[]>([]);
  const running = useRef(0);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    for (const chat of chats) {
      if (attempted.current.has(chat.id) || histories.has(chat.id)) continue;
      attempted.current.add(chat.id);
      queue.current.push(chat.id);
    }

    const preload = async (chatId: string): Promise<void> => {
      const epoch = histories.epoch;
      try {
        const chat = await chatsApi.getChat(chatId);
        if (epoch === histories.epoch && !histories.has(chatId)) histories.remember(chat);
      } catch {
        attempted.current.delete(chatId);
      }
    };

    const next = (): void => {
      while (mounted.current && running.current < PRELOADS_AT_ONCE && queue.current.length > 0) {
        const chatId = queue.current.shift() as string;
        running.current += 1;
        void preload(chatId).finally(() => {
          running.current -= 1;
          next();
        });
      }
    };
    next();
  }, [chats]);
}
