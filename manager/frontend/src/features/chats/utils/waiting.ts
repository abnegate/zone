import type { ChatActivity, ChatActivityReading } from '../types';
import { ACTIVITY_LABELS } from './indicators';

/**
 * Why a chat has stopped and is waiting for its reader.
 *
 * `finished` is the turn ending with nothing outstanding — the reply is
 * written, and whether the reader wants to read it is up to them. The other two
 * are the chat unable to go any further until the reader says something.
 */
export type WaitingReason = 'finished' | 'awaiting_answer' | 'awaiting_approval';

/** One chat that has just stopped, as a notification would describe it. */
export interface ChatWaiting {
  chat_id: string;
  title: string;
  reason: WaitingReason;
}

/** What each reason says in a notification. */
export const WAITING_BODIES: Record<WaitingReason, string> = {
  finished: 'Finished its reply',
  awaiting_answer: ACTIVITY_LABELS.awaiting_answer,
  awaiting_approval: ACTIVITY_LABELS.awaiting_approval,
};

/** What a chat with no title of its own is called. */
export const UNTITLED_CHAT = 'Untitled chat';

/** The readings by chat id, as the watcher remembers them between polls. */
export function activityByChat(
  readings: readonly ChatActivityReading[]
): Record<string, ChatActivity> {
  return Object.fromEntries(readings.map((reading) => [reading.chat_id, reading.activity]));
}

/**
 * Why this chat is waiting, or null if it is not waiting at all.
 *
 * A chat still writing is not waiting for anybody.
 */
function reason(activity: ChatActivity): WaitingReason | null {
  switch (activity) {
    case 'writing':
      return null;
    case 'idle':
      return 'finished';
    case 'awaiting_answer':
      return 'awaiting_answer';
    case 'awaiting_approval':
      return 'awaiting_approval';
  }
}

/**
 * The chats that stopped writing between these two readings.
 *
 * Only a chat the previous reading saw writing counts, which is what makes this
 * the moment the chat stopped rather than the fact that it is stopped now. A
 * chat the previous reading had never seen is not a transition: on the first
 * poll of a session every idle chat in the workspace would otherwise announce
 * itself, and none of them did anything.
 *
 * A chat that moves between two waiting states — a question answered by
 * approving a tool call, say — is not a transition either. It had already
 * stopped, and the reader was already told.
 */
export function waitingTransitions(
  before: Record<string, ChatActivity>,
  after: readonly ChatActivityReading[]
): ChatWaiting[] {
  const waiting: ChatWaiting[] = [];
  for (const reading of after) {
    if (before[reading.chat_id] !== 'writing') continue;
    const why = reason(reading.activity);
    if (!why) continue;
    waiting.push({
      chat_id: reading.chat_id,
      title: reading.title.trim() || UNTITLED_CHAT,
      reason: why,
    });
  }
  return waiting;
}

/** What the notification about a waiting chat says. */
export function waitingNotification(waiting: ChatWaiting): { title: string; body: string } {
  return { title: waiting.title, body: WAITING_BODIES[waiting.reason] };
}
