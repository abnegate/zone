import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type { ChatPullRequest, PullRequestChange } from '../types';

export const PULL_REQUEST_REFRESH_INTERVAL = 60_000;

/**
 * A turn records the pull request it opened just after its stream ends, so
 * the list is read once more this long after.
 */
export const RECORDING_SETTLE_DELAY = 3_000;

export interface ChatPullRequests {
  pullRequests: ChatPullRequest[];
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

function byCreation(left: ChatPullRequest, right: ChatPullRequest): number {
  return left.created_at.localeCompare(right.created_at);
}

export function useChatPullRequests(chatId: string | null, streaming: boolean): ChatPullRequests {
  const [pullRequests, setPullRequests] = useState<ChatPullRequest[]>([]);
  const [dismissed, setDismissed] = useState<ChatPullRequest | null>(null);
  const [pending, setPending] = useState<ReadonlySet<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const read = useRef(0);
  const generation = useRef(0);
  const wasStreaming = useRef(streaming);
  const watching = pullRequests.length > 0;

  const refresh = useCallback(async () => {
    const current = ++read.current;
    if (!chatId) {
      setPullRequests([]);
      return;
    }
    try {
      const loaded = await chatsApi.getChatPullRequests(chatId);
      if (current !== read.current) return;
      setPullRequests(loaded);
      setError(null);
    } catch (failure) {
      if (current !== read.current) return;
      setError(message(failure, 'Failed to load pull requests'));
    }
  }, [chatId]);

  useEffect(() => {
    generation.current += 1;
    setPullRequests([]);
    setDismissed(null);
    setPending(new Set());
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
    }, PULL_REQUEST_REFRESH_INTERVAL);
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
      const current = generation.current;
      mark(pullRequest.id, true);
      setError(null);
      try {
        const updated = await chatsApi.updateChatPullRequest(chatId, pullRequest.id, requested);
        if (current !== generation.current) return;
        read.current += 1;
        if (updated.dismissed) {
          setPullRequests((rows) => rows.filter((row) => row.id !== updated.id));
          setDismissed(updated);
          return;
        }
        setDismissed((last) => (last?.id === updated.id ? null : last));
        setPullRequests((rows) =>
          rows.some((row) => row.id === updated.id)
            ? rows.map((row) => (row.id === updated.id ? updated : row))
            : [...rows, updated].sort(byCreation)
        );
      } catch (failure) {
        if (current !== generation.current) return;
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
