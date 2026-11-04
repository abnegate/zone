import { describe, expect, it } from 'bun:test';
import { reloadOnStaleChunk } from './staleChunk';

function harness(start = 100_000) {
  const listeners: Array<(event: Event) => void> = [];
  const values = new Map<string, string>();
  const state = { reloads: 0, now: start };
  const target = {
    addEventListener: (_type: string, listener: (event: Event) => void) => {
      listeners.push(listener);
    },
    location: {
      reload: () => {
        state.reloads += 1;
      },
    },
  };
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
  };
  reloadOnStaleChunk(target as never, storage, () => state.now);
  const fire = () => {
    const event = new Event('vite:preloadError', { cancelable: true });
    for (const listener of listeners) {
      listener(event);
    }
    return event;
  };
  return { state, fire };
}

describe('reloadOnStaleChunk', () => {
  it('reloads and lets the import reject when a chunk fails to load', () => {
    const { state, fire } = harness();
    const event = fire();
    expect(state.reloads).toBe(1);
    expect(event.defaultPrevented).toBe(false);
  });

  it('lets a repeat failure surface instead of looping', () => {
    const { state, fire } = harness();
    fire();
    state.now += 1_000;
    fire();
    expect(state.reloads).toBe(1);
  });

  it('reloads again once the cooldown has passed', () => {
    const { state, fire } = harness();
    fire();
    state.now += 61_000;
    fire();
    expect(state.reloads).toBe(2);
  });
});
