import { SUBAGENT_FRAME } from '../subagentSchemas';

export type FrameListener = (frame: unknown) => void;

/** Frame types only bus subscribers read, so the chat socket neither applies nor holds them. */
const SUBSCRIBER_FRAMES: ReadonlySet<unknown> = new Set([SUBAGENT_FRAME]);

const listeners = new Map<string, Set<FrameListener>>();

function frameType(frame: unknown): unknown {
  return typeof frame === 'object' && frame !== null ? (frame as { type?: unknown }).type : null;
}

/**
 * Hands a chat socket frame to everything subscribed to that chat, and says
 * whether the frame was meant for subscribers alone.
 */
export function publishFrame(chatId: string, frame: unknown): boolean {
  const subscribed = listeners.get(chatId);
  if (subscribed) {
    for (const listener of [...subscribed]) {
      try {
        listener(frame);
      } catch (failure) {
        // One broken subscriber must not stop the socket handler or its siblings.
        queueMicrotask(() => {
          throw failure;
        });
      }
    }
  }
  return SUBSCRIBER_FRAMES.has(frameType(frame));
}

export function subscribeFrames(chatId: string, listener: FrameListener): () => void {
  let subscribed = listeners.get(chatId);
  if (!subscribed) {
    subscribed = new Set();
    listeners.set(chatId, subscribed);
  }
  const owned = subscribed;
  owned.add(listener);
  return () => {
    owned.delete(listener);
    if (owned.size === 0 && listeners.get(chatId) === owned) listeners.delete(chatId);
  };
}
