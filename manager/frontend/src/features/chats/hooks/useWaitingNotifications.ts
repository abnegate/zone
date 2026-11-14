import { useEffect, useRef } from 'react';
import { chatsApi } from '../../../api/chats';
import { notificationsAllowed, notifyAboutChat } from '../../../shared/notifications';
import type { ChatActivity, ChatActivityReading } from '../types';
import { activityByChat, waitingNotification, waitingTransitions } from '../utils/waiting';

/**
 * How often the watcher asks what the workspace's chats are doing.
 *
 * Unlike the chat list, this keeps asking while the page is hidden, which is
 * the whole point of it: a reader who is looking at the console does not need
 * telling that a chat stopped, and a reader who went to do something else is
 * the one being notified.
 */
export const WATCH_INTERVAL = 20_000;

/**
 * Notifies the reader when a chat stops writing and starts waiting for them.
 *
 * The transition is read by comparing two polls, so a chat has to be seen
 * writing before it is reported as stopped. A turn that starts and ends between
 * two polls is therefore never notified: the watcher did not see it writing,
 * and saying a chat stopped when it was never seen running would also announce
 * every idle chat in the workspace on the first poll of a session.
 *
 * Nothing is read at all until the platform says a notification would reach the
 * reader, so a reader who has refused notifications is not polled for.
 */
export function useWaitingNotifications(
  workspaceId: string | null | undefined,
  openChatId: string | null
): void {
  const seen = useRef<Record<string, ChatActivity> | null>(null);
  const open = useRef(openChatId);
  open.current = openChatId;

  useEffect(() => {
    seen.current = null;
    if (!workspaceId) return;
    let stopped = false;
    let timer: ReturnType<typeof setInterval> | undefined;

    const poll = async () => {
      let readings: ChatActivityReading[];
      try {
        readings = await chatsApi.getChatActivity(workspaceId, false);
      } catch {
        // A failed read is not a transition. The readings already in hand stay,
        // so a chat that stops while the server is unreachable is still
        // reported by the poll that gets through.
        return;
      }
      if (stopped) return;
      const before = seen.current;
      seen.current = activityByChat(readings);
      if (!before) return;
      for (const waiting of waitingTransitions(before, readings)) {
        // The reader is looking at this chat, in a window that has the focus.
        // They can see it stop.
        if (waiting.chat_id === open.current && document.hasFocus()) continue;
        const { title, body } = waitingNotification(waiting);
        void notifyAboutChat({ chatId: waiting.chat_id, title, body });
      }
    };

    void (async () => {
      const allowed = await notificationsAllowed();
      if (!allowed || stopped) return;
      await poll();
      if (stopped) return;
      timer = setInterval(() => void poll(), WATCH_INTERVAL);
    })();

    return () => {
      stopped = true;
      if (timer) clearInterval(timer);
    };
  }, [workspaceId]);
}
