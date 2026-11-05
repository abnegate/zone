import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type { ChatTask } from '../types';
import { taskActive } from '../utils/tasks';
import { RECORDING_SETTLE_DELAY } from './useChatPullRequests';

/** How often the tasks are read while one is running or the chat's agent may start one. */
export const TASK_REFRESH_INTERVAL = 5_000;

export interface ChatTasks {
  tasks: ChatTask[];
  error: string | null;
  refresh: () => Promise<void>;
}

export function useChatTasks(chatId: string | null, streaming: boolean): ChatTasks {
  const [tasks, setTasks] = useState<ChatTask[]>([]);
  const [error, setError] = useState<string | null>(null);
  const read = useRef(0);
  const wasStreaming = useRef(streaming);
  const watching = streaming || tasks.some(taskActive);

  const refresh = useCallback(async () => {
    const current = ++read.current;
    if (!chatId) {
      setTasks([]);
      return;
    }
    try {
      const loaded = await chatsApi.getChatTasks(chatId);
      if (current !== read.current) return;
      setTasks(loaded);
      setError(null);
    } catch (failure) {
      if (current !== read.current) return;
      setError(failure instanceof Error ? failure.message : 'Failed to load tasks');
    }
  }, [chatId]);

  useEffect(() => {
    setTasks([]);
    setError(null);
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

  return { tasks, error, refresh };
}
