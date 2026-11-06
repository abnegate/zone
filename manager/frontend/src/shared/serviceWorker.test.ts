import { describe, expect, it } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  SERVICE_WORKER_FILE,
  SERVICE_WORKER_IDLE_TIMEOUT_MILLISECONDS,
  SERVICE_WORKER_NAVIGATION_DENYLIST,
  setUpServiceWorker,
} from './serviceWorker';

function host({
  hostname = 'zone.example.com',
  readyState = 'complete',
  idle = true,
}: {
  hostname?: string;
  readyState?: DocumentReadyState;
  idle?: boolean;
} = {}) {
  const registered: string[] = [];
  const unregistered: string[] = [];
  const idleCallbacks: { callback: () => void; timeout?: number }[] = [];
  const timers: (() => void)[] = [];
  const listeners = new Map<string, () => void>();
  const fake = {
    document: { readyState },
    location: { hostname },
    navigator: {
      serviceWorker: {
        register: (url: string) => {
          registered.push(url);
          return Promise.resolve({} as ServiceWorkerRegistration);
        },
        getRegistrations: () =>
          Promise.resolve([
            {
              unregister: () => {
                unregistered.push('registration');
                return Promise.resolve(true);
              },
            } as unknown as ServiceWorkerRegistration,
          ]),
      },
    },
    addEventListener: (type: string, listener: () => void) => {
      listeners.set(type, listener);
    },
    setTimeout: (callback: () => void) => {
      timers.push(callback);
      return 0;
    },
    requestIdleCallback: idle
      ? (callback: () => void, options?: { timeout?: number }) => {
          idleCallbacks.push({ callback, timeout: options?.timeout });
          return 0;
        }
      : undefined,
  };
  return {
    fake: fake as unknown as Parameters<typeof setUpServiceWorker>[0],
    registered,
    unregistered,
    idleCallbacks,
    timers,
    listeners,
  };
}

describe('setUpServiceWorker', () => {
  it('registers only once the page is idle, with a deadline', () => {
    const { fake, registered, idleCallbacks } = host();
    setUpServiceWorker(fake);
    expect(registered).toEqual([]);
    expect(idleCallbacks[0].timeout).toBe(SERVICE_WORKER_IDLE_TIMEOUT_MILLISECONDS);
    idleCallbacks[0].callback();
    expect(registered).toEqual([`/${SERVICE_WORKER_FILE}`]);
  });

  it('waits for load before looking for idle time', () => {
    const { fake, registered, idleCallbacks, listeners } = host({ readyState: 'interactive' });
    setUpServiceWorker(fake);
    expect(idleCallbacks).toHaveLength(0);
    listeners.get('load')?.();
    idleCallbacks[0].callback();
    expect(registered).toEqual([`/${SERVICE_WORKER_FILE}`]);
  });

  it('falls back to a timer where idle callbacks are missing', () => {
    const { fake, registered, timers } = host({ idle: false });
    setUpServiceWorker(fake);
    expect(registered).toEqual([]);
    timers[0]();
    expect(registered).toEqual([`/${SERVICE_WORKER_FILE}`]);
  });

  it('removes service workers instead on a local host', async () => {
    for (const hostname of ['localhost', '127.0.0.1']) {
      const { fake, registered, unregistered, idleCallbacks } = host({ hostname });
      setUpServiceWorker(fake);
      await Promise.resolve();
      await Promise.resolve();
      expect(unregistered).toEqual(['registration']);
      expect(idleCallbacks).toHaveLength(0);
      expect(registered).toEqual([]);
    }
  });
});

describe('SERVICE_WORKER_NAVIGATION_DENYLIST', () => {
  const denied = (path: string) =>
    SERVICE_WORKER_NAVIGATION_DENYLIST.some((pattern) => pattern.test(path));

  it('lets GitHub callback navigations reach the server', () => {
    expect(denied('/api/github/callbacks/manifest?code=x&state=y')).toBe(true);
    expect(denied('/api/github/callbacks/setup?installation_id=1')).toBe(true);
  });

  it('keeps serving app routes from the cached shell', () => {
    expect(denied('/org-settings?tab=github')).toBe(false);
    expect(denied('/sources')).toBe(false);
  });

  it('is the navigation fallback denylist the worker is built with', () => {
    const config = readFileSync(join(import.meta.dir, '..', '..', 'vite.config.ts'), 'utf8');
    expect(config).toContain('navigateFallbackDenylist: SERVICE_WORKER_NAVIGATION_DENYLIST');
  });
});
