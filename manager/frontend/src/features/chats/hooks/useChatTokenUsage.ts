import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type { TokenTotals } from '../types';
import { RECORDING_SETTLE_DELAY } from './useChatPullRequests';

interface Reading {
  chatId: string | null;
  totals: TokenTotals | null;
}

const NOTHING_READ: Reading = { chatId: null, totals: null };

/**
 * What the chat has spent, read once per chat and again when a turn finishes.
 * A turn's spend is stored just after its stream ends, so the read waits for
 * that to land, even when the chat is left and reopened meanwhile. A failed
 * read keeps the last totals.
 */
export function useChatTokenUsage(chatId: string | null, streaming: boolean): TokenTotals | null {
  const [reading, setReading] = useState<Reading>(NOTHING_READ);
  const read = useRef(0);
  const wasStreaming = useRef(streaming);
  const settling = useRef(new Map<string, number>());

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
    const ended = wasStreaming.current && !streaming;
    wasStreaming.current = streaming;
    if (!chatId) return;
    if (ended) settling.current.set(chatId, Date.now() + RECORDING_SETTLE_DELAY);
    const deadline = settling.current.get(chatId);
    if (deadline === undefined) return;
    const remaining = deadline - Date.now();
    if (remaining <= 0) {
      settling.current.delete(chatId);
      return;
    }
    const settled = setTimeout(() => {
      settling.current.delete(chatId);
      void refresh();
    }, remaining);
    return () => clearTimeout(settled);
  }, [chatId, streaming, refresh]);

  return reading.chatId === chatId ? reading.totals : null;
}
