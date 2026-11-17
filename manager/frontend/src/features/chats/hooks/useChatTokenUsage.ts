import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type { ChatTokenUsage } from '../types';

/**
 * How many of the chat's turns the panel asks for. Enough to show the recent
 * shape of a conversation without pulling a long one's whole history: the
 * totals cover every turn regardless of how many are listed.
 */
export const TOKEN_TURNS = 20;

export interface ChatTokens {
  usage: ChatTokenUsage | null;
  error: string | null;
  refresh: () => Promise<void>;
}

interface Reading {
  chatId: string | null;
  usage: ChatTokenUsage | null;
  error: string | null;
}

const NOTHING_READ: Reading = { chatId: null, usage: null, error: null };

/**
 * What the chat has spent, read once per chat and again when a turn finishes.
 *
 * The row is written as the turn closes, so a read while `streaming` is true
 * would miss the turn in flight. Waiting for streaming to end means the figure
 * lands a moment after the answer rather than during it, which is the right
 * trade: a number that ticks up mid-answer invites watching it instead of
 * reading the reply.
 */
export function useChatTokenUsage(chatId: string | null, streaming: boolean): ChatTokens {
  const [reading, setReading] = useState<Reading>(NOTHING_READ);
  const read = useRef(0);
  const wasStreaming = useRef(streaming);
  const current = reading.chatId === chatId ? reading : NOTHING_READ;

  const refresh = useCallback(async () => {
    const attempt = ++read.current;
    if (!chatId) {
      setReading(NOTHING_READ);
      return;
    }
    try {
      const loaded = await chatsApi.getChatTokenUsage(chatId, TOKEN_TURNS);
      if (attempt !== read.current) return;
      setReading({ chatId, usage: loaded, error: null });
    } catch (failure) {
      if (attempt !== read.current) return;
      const error = failure instanceof Error ? failure.message : 'Failed to load token usage';
      setReading((last) => ({
        chatId,
        usage: last.chatId === chatId ? last.usage : null,
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
  }, [streaming, refresh]);

  return { usage: current.usage, error: current.error, refresh };
}
