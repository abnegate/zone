import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type { ChatTask } from '../types';
import { cached } from '../utils/cached';
import { taskActive } from '../utils/tasks';
import { RECORDING_SETTLE_DELAY } from './useChatPullRequests';

/** How often the tasks are read while one is running or the chat's agent may start one. */
export const TASK_REFRESH_INTERVAL = 5_000;

export interface ChatTasks {
  tasks: ChatTask[];
  error: string | null;
  refresh: () => Promise<void>;
}

interface Reading {
  chatId: string | null;
  tasks: ChatTask[];
  error: string | null;
}

const NOTHING_READ: Reading = { chatId: null, tasks: [], error: null };

function remembered(chatId: string | null): Reading {
  if (!chatId) return NOTHING_READ;
  return { chatId, tasks: cached.tasks.peek(chatId) ?? NOTHING_READ.tasks, error: null };
}

export function useChatTasks(chatId: string | null, streaming: boolean): ChatTasks {
  const [reading, setReading] = useState<Reading>(NOTHING_READ);
  const read = useRef(0);
  const wasStreaming = useRef(streaming);
  const current = reading.chatId === chatId ? reading : remembered(chatId);
  const watching = streaming || current.tasks.some(taskActive);

  const refresh = useCallback(async () => {
    const attempt = ++read.current;
    if (!chatId) {
      setReading(NOTHING_READ);
      return;
    }
    try {
      const loaded = await chatsApi.getChatTasks(chatId);
      if (attempt !== read.current) return;
      cached.tasks.remember(chatId, loaded);
      setReading({ chatId, tasks: loaded, error: null });
    } catch (failure) {
      if (attempt !== read.current) return;
      const error = failure instanceof Error ? failure.message : 'Failed to load tasks';
      setReading((last) => ({
        chatId,
        tasks: last.chatId === chatId ? last.tasks : remembered(chatId).tasks,
        error,
      }));
    }
  }, [chatId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (wasStreaming.current === streaming) return;
    wasStreaming.current = streaming;
    if (streaming) return;
    void refresh();
    const settled = setTimeout(() => void refresh(), RECORDING_SETTLE_DELAY);
    return () => clearTimeout(settled);
  }, [streaming, refresh]);

  useEffect(() => {
    if (!watching) return;
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') void refresh();
    }, TASK_REFRESH_INTERVAL);
    return () => clearInterval(timer);
  }, [watching, refresh]);

  return { tasks: current.tasks, error: current.error, refresh };
}
