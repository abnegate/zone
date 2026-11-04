import { describe, expect, it } from 'bun:test';
import { ACCESS_TOKEN_KEY, REFRESH_TOKEN_KEY } from './api/session';
import { preloadRoute, preloadSignedInRoutes } from './routes';

const storageWith = (...keys: string[]) => ({
  getItem: (key: string) => (keys.includes(key) ? 'token' : null),
});

describe('route preloading', () => {
  it('loads nothing for a signed out visitor on a protected path', async () => {
    expect(await preloadRoute('/models', storageWith())).toEqual([]);
    expect(await preloadSignedInRoutes(storageWith())).toEqual([]);
  });

  it('loads nothing for the eager signed out forms', async () => {
    for (const path of ['/login', '/register', '/forgot-password', '/unauthorized']) {
      expect(await preloadRoute(path, storageWith(ACCESS_TOKEN_KEY))).toEqual([]);
    }
  });

  it('loads the shell, and the page when a session is stored', async () => {
    expect(await preloadRoute('/models', storageWith(ACCESS_TOKEN_KEY))).toHaveLength(4);
    expect(await preloadRoute('/models/', storageWith(REFRESH_TOKEN_KEY))).toHaveLength(4);
    expect(await preloadRoute('/', storageWith(ACCESS_TOKEN_KEY))).toHaveLength(4);
  });

  it('loads each signed in chunk once for later navigation', async () => {
    const settled = (await preloadSignedInRoutes(storageWith(ACCESS_TOKEN_KEY))) as unknown[];
    expect(settled).toHaveLength(14);
  });
});
