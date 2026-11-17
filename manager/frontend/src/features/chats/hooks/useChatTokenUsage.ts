import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type { TokenTotals } from '../types';
import { RECORDING_SETTLE_DELAY } from './useChatPullRequests';

interface Reading {
  chatId: string | null;
  totals: TokenTotals | null;
}

const NOTHING_READ: Reading = { chatId: null, totals: null };

export const LATE_RECORDING_DELAY = 10_000;

const RECORDING_READS = [RECORDING_SETTLE_DELAY, LATE_RECORDING_DELAY];
const LAST_READ = Math.max(...RECORDING_READS);

/**
 * What the chat has spent, read once per chat and again after each turn
 * finishes. A turn's spend is stored just after its stream ends, so it is read
 * once the write has had time to land and once more later for a slow one, even
 * when the chat is left and reopened meanwhile. A failed read keeps the last
 * totals. Leaving a chat mid-stream does not end its turn, which the server
 * finishes unobserved, so reopening such a chat starts the same reads from then.
 */
export function useChatTokenUsage(chatId: string | null, streaming: boolean): TokenTotals | null {
  const [reading, setReading] = useState<Reading>(NOTHING_READ);
  const read = useRef(0);
  const streamingChat = useRef<string | null>(streaming ? chatId : null);
  const endings = useRef(new Map<string, number>());
  const departed = useRef(new Set<string>());

  const refresh = useCallback(async () => {
    const attempt = ++read.current;
    if (!chatId) {
      setReading(NOTHING_READ);
      return;
    }
    try {
      const totals = await chatsApi.getChatTokenTotals(chatId);
      if (attempt !== read.current) return;
      setReading({ chatId, totals });
    } catch {
      if (attempt !== read.current) return;
      setReading((last) => (last.chatId === chatId ? last : { chatId, totals: null }));
    }
  }, [chatId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    const now = Date.now();
    const streamed = streamingChat.current;
    streamingChat.current = streaming ? chatId : null;
    if (streamed && streamed !== chatId) departed.current.add(streamed);
    else if (streamed && !streaming) endings.current.set(streamed, now);
    if (chatId && departed.current.delete(chatId)) endings.current.set(chatId, now);
    for (const [ended, at] of endings.current) {
      if (now - at >= LAST_READ) endings.current.delete(ended);
    }

    const ended = chatId ? endings.current.get(chatId) : undefined;
    if (ended === undefined) return;
    const timers = RECORDING_READS.map((delay) => ended + delay - now)
      .filter((remaining) => remaining > 0)
      .map((remaining) => setTimeout(() => void refresh(), remaining));
    return () => timers.forEach(clearTimeout);
  }, [chatId, streaming, refresh]);

  return reading.chatId === chatId ? reading.totals : null;
}
