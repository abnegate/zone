import { describe, expect, it } from 'bun:test';
import type { createRoot } from 'react-dom/client';
import { type BootSteps, boot } from './boot';

const fakeCreateRoot = (() => ({})) as unknown as typeof createRoot;

function steps(overrides: Partial<BootSteps> = {}) {
  const calls: string[] = [];
  const using: BootSteps = {
    route: (pathname) => {
      calls.push(`route ${pathname}`);
    },
    renderer: async () => ({ createRoot: fakeCreateRoot }),
    app: async () => ({
      start: (create) => {
        calls.push(create === fakeCreateRoot ? 'start' : 'start with another renderer');
      },
    }),
    fail: () => {
      calls.push('fail');
    },
    ...overrides,
  };
  return { using, calls };
}

describe('boot', () => {
  it('starts the app with the renderer once both have loaded', async () => {
    const { using, calls } = steps();
    await boot('/login', using);
    expect(calls).toEqual(['route /login', 'start']);
  });

  it('shows the failure when a chunk fails to load', async () => {
    for (const failing of ['renderer', 'app'] as const) {
      const { using, calls } = steps({ [failing]: () => Promise.reject(new Error('offline')) });
      await boot('/login', using);
      expect(calls).toEqual(['route /login', 'fail']);
    }
  });

  it('shows the failure when starting throws before anything loads', async () => {
    const { using, calls } = steps({
      route: () => {
        throw new Error('storage unavailable');
      },
    });
    await boot('/login', using);
    expect(calls).toEqual(['fail']);
  });

  it('shows the failure when the app throws while starting', async () => {
    const { using, calls } = steps({
      app: async () => ({
        start: () => {
          throw new Error('no root');
        },
      }),
    });
    await boot('/login', using);
    expect(calls).toEqual(['route /login', 'fail']);
  });
});
