import { vi } from 'bun:test';

/**
 * Starts fake timers once no zero-delay timeout is left pending.
 *
 * happy-dom runs every zero-delay timeout of a window from one shared timer, and Bun suspends a
 * timer that is already pending when fake timers start. A timeout the previous test left behind
 * would keep that shared timer open for the whole faked test, and every zero-delay timeout
 * scheduled in it would wait until real timers return.
 */
export async function fakeTimers(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
  vi.useFakeTimers();
}
