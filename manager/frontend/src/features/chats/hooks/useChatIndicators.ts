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
  streaming = false
): ChatIndicators {
  const [indicators, setIndicators] = useState<Record<string, ChatIndicator>>({});
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const read = useRef(0);
  const wasStreaming = useRef(streaming);

  const refresh = useCallback(async () => {
    const current = ++read.current;
    if (!workspaceId) {
      setIndicators({});
      return;
    }
    setLoading(true);
    try {
      const loaded = await chatsApi.getChatIndicators(workspaceId, archived);
      if (current !== read.current) return;
      setIndicators(byChat(loaded));
      setError(null);
    } catch (failure) {
      if (current !== read.current) return;
      setError(message(failure));
    } finally {
      if (current === read.current) setLoading(false);
    }
  }, [workspaceId, archived]);

  useEffect(() => {
    setIndicators({});
    setError(null);
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

  return { indicators, loading, error, refresh };
}
