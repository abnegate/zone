import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type { ChatPullRequest, PullRequestChange } from '../types';

/** How often an open panel asks again. The server answers from a short cache between. */
export const PULL_REQUEST_REFRESH_MS = 60_000;

export interface ChatPullRequests {
  pullRequests: ChatPullRequest[];
  /** The one just dismissed, which can still be brought back. */
  dismissed: ChatPullRequest | null;
  pending: ReadonlySet<string>;
  error: string | null;
  refresh: () => Promise<void>;
  change: (pullRequest: ChatPullRequest, change: PullRequestChange) => Promise<void>;
  undo: () => Promise<void>;
}

function message(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}

/**
 * The pull requests a chat opened. Loaded per chat, again when a turn ends
 * since a turn is what opens one, and every minute while there are any to
 * watch and the page is in view.
 */
export function useChatPullRequests(chatId: string | null, streaming: boolean): ChatPullRequests {
  const [pullRequests, setPullRequests] = useState<ChatPullRequest[]>([]);
  const [dismissed, setDismissed] = useState<ChatPullRequest | null>(null);
  const [pending, setPending] = useState<ReadonlySet<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);
  const wasStreaming = useRef(streaming);
  const watching = pullRequests.length > 0;

  const refresh = useCallback(async () => {
    const current = ++request.current;
    if (!chatId) {
      setPullRequests([]);
      return;
    }
    try {
      const loaded = await chatsApi.getChatPullRequests(chatId);
      if (current !== request.current) return;
      setPullRequests(loaded);
      setError(null);
    } catch (failure) {
      if (current !== request.current) return;
      setError(message(failure, 'Failed to load pull requests'));
    }
  }, [chatId]);

  useEffect(() => {
    setPullRequests([]);
    setDismissed(null);
    setPending(new Set());
    setError(null);
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (wasStreaming.current === streaming) return;
    wasStreaming.current = streaming;
    if (!streaming) void refresh();
  }, [streaming, refresh]);

  useEffect(() => {
    if (!watching) return;
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') void refresh();
    }, PULL_REQUEST_REFRESH_MS);
    return () => clearInterval(timer);
  }, [watching, refresh]);

  const mark = useCallback(
    (id: string, busy: boolean) =>
      setPending((current) => {
        const next = new Set(current);
        if (busy) next.add(id);
        else next.delete(id);
        return next;
      }),
    []
  );

  const change = useCallback(
    async (pullRequest: ChatPullRequest, requested: PullRequestChange) => {
      if (!chatId) return;
      const current = request.current;
      mark(pullRequest.id, true);
      setError(null);
      try {
        const updated = await chatsApi.updateChatPullRequest(chatId, pullRequest.id, requested);
        if (current !== request.current) return;
        if (updated.dismissed) {
          setPullRequests((rows) => rows.filter((row) => row.id !== updated.id));
          setDismissed(updated);
          return;
        }
        setDismissed((last) => (last?.id === updated.id ? null : last));
        setPullRequests((rows) =>
          rows.some((row) => row.id === updated.id)
            ? rows.map((row) => (row.id === updated.id ? updated : row))
            : [...rows, updated].sort((left, right) =>
                left.created_at.localeCompare(right.created_at)
              )
        );
      } catch (failure) {
        if (current !== request.current) return;
        setError(message(failure, 'Failed to update the pull request'));
      } finally {
        mark(pullRequest.id, false);
      }
    },
    [chatId, mark]
  );

  const undo = useCallback(async () => {
    if (dismissed) await change(dismissed, { dismissed: false });
  }, [change, dismissed]);

  return { pullRequests, dismissed, pending, error, refresh, change, undo };
}
