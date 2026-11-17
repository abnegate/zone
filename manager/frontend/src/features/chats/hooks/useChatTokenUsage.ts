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
 * that to land. A failed read keeps the last totals.
 */
export function useChatTokenUsage(chatId: string | null, streaming: boolean): TokenTotals | null {
  const [reading, setReading] = useState<Reading>(NOTHING_READ);
  const read = useRef(0);
  const wasStreaming = useRef(streaming);

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
    if (wasStreaming.current === streaming) return;
    wasStreaming.current = streaming;
    if (streaming) return;
    const settled = setTimeout(() => void refresh(), RECORDING_SETTLE_DELAY);
    return () => clearTimeout(settled);
  }, [streaming, refresh]);

  return reading.chatId === chatId ? reading.totals : null;
}
