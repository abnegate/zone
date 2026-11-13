import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type { ChatIndicator } from '../types';
import { byChat } from '../utils/indicators';

/**
 * How often the list asks what its chats are doing.
 *
 * Only while the tab is visible: a list nobody is looking at is not worth a
 * request, and the first read on return happens immediately.
 */
export const INDICATOR_REFRESH_INTERVAL = 20_000;

/** A turn records what it opened just after its stream ends. */
export const SETTLE_DELAY = 3_000;

export interface ChatIndicators {
  indicators: Record<string, ChatIndicator>;
  loading: boolean;
  /**
   * Whether a read has settled, however it went. A filter judges chats on
   * these readings, so it has nothing to say until one has landed — and an
   * empty answer is a landed one.
   */
  loaded: boolean;
  error: string | null;
  refresh: () => Promise<void>;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : 'Failed to load chat indicators';
}

/**
 * What each chat in the workspace is doing, kept current for the list.
 *
 * A failed read leaves the last readings in place and reports the error: a
 * blank row would say the chats are idle, which is not what a failed request
 * found out.
 */
export function useChatIndicators(
  workspaceId: string | null | undefined,
  archived: boolean,
  streaming = false,
  /**
   * How many chats the paged list is holding. Readings are asked for over
   * that slice, asked for again when another page lands, and not asked for at
   * all until the list has rows.
   */
  listed = 0
): ChatIndicators {
  const [indicators, setIndicators] = useState<Record<string, ChatIndicator>>({});
  const [loading, setLoading] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const read = useRef(0);
  const wasStreaming = useRef(streaming);
  /** Which list the readings in hand belong to. */
  const listing = useRef(`${workspaceId ?? ''}:${archived}`);

  const refresh = useCallback(async () => {
    const current = ++read.current;
    if (!workspaceId) {
      setIndicators({});
      setLoaded(true);
      return;
    }
    // Nothing listed yet is nothing to read for, and asking anyway would ask
    // for every chat in the workspace — the whole point of following the
    // list's own page.
    if (listed === 0) return;
    setLoading(true);
    try {
      const loaded = await chatsApi.getChatIndicators(workspaceId, archived, listed);
      if (current !== read.current) return;
      setIndicators(byChat(loaded));
      setError(null);
    } catch (failure) {
      if (current !== read.current) return;
      setError(message(failure));
    } finally {
      if (current === read.current) {
        setLoading(false);
        setLoaded(true);
      }
    }
  }, [workspaceId, archived, listed]);

  // A different workspace or tab is a different list, so its readings start
  // again. A further page of the same list is not: blanking the map there
  // would flicker every row that already had a reading back to nothing.
  const scope = `${workspaceId ?? ''}:${archived}`;
  if (listing.current !== scope) {
    listing.current = scope;
    setIndicators({});
    setLoaded(false);
    setError(null);
  }

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // A turn that just ended changes what its row says, and may have opened a
  // pull request the server records a moment later.
  useEffect(() => {
    if (wasStreaming.current === streaming) return;
    wasStreaming.current = streaming;
    void refresh();
    if (streaming) return;
    const settled = setTimeout(() => void refresh(), SETTLE_DELAY);
    return () => clearTimeout(settled);
  }, [streaming, refresh]);

  useEffect(() => {
    if (!workspaceId) return;
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') void refresh();
    }, INDICATOR_REFRESH_INTERVAL);
    const woken = () => {
      if (document.visibilityState === 'visible') void refresh();
    };
    document.addEventListener('visibilitychange', woken);
    return () => {
      clearInterval(timer);
      document.removeEventListener('visibilitychange', woken);
    };
  }, [workspaceId, refresh]);

  return { indicators, loading, loaded, error, refresh };
}
