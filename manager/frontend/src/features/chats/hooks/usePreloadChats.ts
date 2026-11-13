import { useEffect, useRef } from 'react';
import { chatsApi } from '../../../api/chats';
import type { Chat } from '../types';
import { type ChatCache, cacheEpoch } from '../utils/cache';
import { cached } from '../utils/cached';

export const PRELOADS_AT_ONCE = 2;

interface Preload {
  missing: (chatId: string) => boolean;
  fill: (chatId: string) => Promise<void>;
}

function preload<T>(cache: ChatCache<T>, load: (chatId: string) => Promise<T>): Preload {
  return {
    missing: (chatId) => !cache.has(chatId),
    fill: async (chatId) => {
      if (cache.has(chatId)) return;
      const epoch = cacheEpoch();
      const value = await load(chatId);
      if (epoch === cacheEpoch() && !cache.has(chatId)) cache.remember(chatId, value);
    },
  };
}

const PRELOADS: readonly Preload[] = [
  preload(cached.histories, (chatId) => chatsApi.getChat(chatId)),
  preload(cached.pullRequests, (chatId) => chatsApi.getChatPullRequests(chatId)),
  preload(cached.tasks, (chatId) => chatsApi.getChatTasks(chatId)),
  preload(cached.subagents, async (chatId) => (await chatsApi.getChatSubagents(chatId)).subagents),
  preload(cached.sources, (chatId) => chatsApi.getChatSources(chatId)),
  preload(cached.providers, (chatId) => chatsApi.getChatProviders(chatId)),
];

/**
 * Loads everything the console shows about every listed chat into the chat
 * caches, a few chats at a time, so opening any chat on a loaded page draws
 * its history and its panel at once. Nothing a cache already holds is fetched
 * again or replaced.
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
      if (attempted.current.has(chat.id)) continue;
      if (!PRELOADS.some((kind) => kind.missing(chat.id))) continue;
      attempted.current.add(chat.id);
      queue.current.push(chat.id);
    }

    const fill = async (chatId: string): Promise<void> => {
      const results = await Promise.allSettled(PRELOADS.map((kind) => kind.fill(chatId)));
      if (results.some((result) => result.status === 'rejected')) {
        attempted.current.delete(chatId);
      }
    };

    const next = (): void => {
      while (mounted.current && running.current < PRELOADS_AT_ONCE && queue.current.length > 0) {
        const chatId = queue.current.shift() as string;
        running.current += 1;
        void fill(chatId).finally(() => {
          running.current -= 1;
          next();
        });
      }
    };
    next();
  }, [chats]);
}
