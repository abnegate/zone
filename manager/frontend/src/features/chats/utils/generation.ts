import { cached } from './cached';

/** When a chat's running turn started, and which reply it is writing. */
export interface GenerationClock {
  /** Epoch milliseconds the turn was first seen running. */
  started: number;
  /** The assistant message being written, once a frame has named one. */
  messageId: string | null;
}

/**
 * The clock for a turn starting here and now, replacing whatever was remembered.
 *
 * A send is always a new turn, so it never inherits the reading an earlier one
 * left behind: a turn that ended while its chat was unselected leaves its clock
 * in the cache, and reusing that would open the counter minutes in.
 */
export function beginGeneration(chatId: string): number {
  const started = Date.now();
  cached.generations.remember(chatId, { started, messageId: null });
  return started;
}

/**
 * The clock for a turn a frame says is running, kept from before when it is the
 * same turn.
 *
 * The same turn means the reply the remembered clock was started for, or no
 * reply named on either side yet — the window between a send and the
 * message_start that says what it produced. Any other reply is a later turn and
 * starts the count again.
 *
 * This is what carries the counter across a chat swap: reselecting a chat
 * mid-turn replays a message_start for the reply still being written, so the
 * counter picks up where the reader left it rather than at zero.
 */
export function resumeGeneration(chatId: string, messageId: string | null): number {
  const remembered = cached.generations.peek(chatId);
  const same =
    remembered !== undefined &&
    (remembered.messageId === null || remembered.messageId === messageId);
  if (!same) {
    const started = Date.now();
    cached.generations.remember(chatId, { started, messageId });
    return started;
  }
  cached.generations.remember(chatId, {
    started: remembered.started,
    messageId: messageId ?? remembered.messageId,
  });
  return remembered.started;
}

/**
 * Forgets a chat's clock, for a turn that ended rather than one that merely
 * went quiet.
 *
 * A dropped socket is not an ending: the server goes on writing the reply and
 * replays it on reconnect, and forgetting the clock there would restart the
 * count on a turn that never stopped.
 */
export function settleGeneration(chatId: string): void {
  cached.generations.forget(chatId);
}

/** What a chat's clock reads on reselecting it, before any frame has arrived. */
export function rememberedGeneration(chatId: string): number | null {
  return cached.generations.peek(chatId)?.started ?? null;
}
