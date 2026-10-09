import { beforeEach, describe, expect, it } from 'bun:test';
import {
  LAST_PATH_KEY,
  lastPathToRestore,
  pathAfterAuth,
  persistLastPath,
  restoreLastPath,
} from './lastPath';

function memoryStorage(initial: Record<string, string> = {}): Storage {
  const data = { ...initial };
  return {
    get length() {
      return Object.keys(data).length;
    },
    clear() {
      for (const key of Object.keys(data)) {
        delete data[key];
      }
    },
    getItem(key: string) {
      return Object.hasOwn(data, key) ? data[key] : null;
    },
    key(index: number) {
      return Object.keys(data)[index] ?? null;
    },
    removeItem(key: string) {
      delete data[key];
    },
    setItem(key: string, value: string) {
      data[key] = value;
    },
  };
}

describe('lastPath', () => {
  let storage: Storage;

  beforeEach(() => {
    persistLastPath('/__reset__', memoryStorage());
    storage = memoryStorage();
  });

  it('persists app routes and skips auth pages', () => {
    persistLastPath('/chats?id=chat-1', storage);
    expect(storage.getItem(LAST_PATH_KEY)).toBe('/chats?id=chat-1');
    persistLastPath('/login', storage);
    expect(storage.getItem(LAST_PATH_KEY)).toBe('/chats?id=chat-1');
  });

  it('restores a saved chat onto a default boot at /', () => {
    storage.setItem(LAST_PATH_KEY, '/chats?id=chat-9');
    const replaced: string[] = [];
    const history = {
      state: null,
      replaceState(_state: unknown, _unused: string, url: string) {
        replaced.push(url);
      },
    };
    expect(restoreLastPath('/', history, storage)).toBe('/chats?id=chat-9');
    expect(replaced).toEqual(['/chats?id=chat-9']);
  });

  it('leaves a restored webview URL alone', () => {
    storage.setItem(LAST_PATH_KEY, '/chats?id=chat-9');
    const history = {
      state: null,
      replaceState() {
        throw new Error('must not replace');
      },
    };
    expect(restoreLastPath('/models', history, storage)).toBe('/models');
  });

  it('returns the interrupted location after auth', () => {
    expect(pathAfterAuth({ pathname: '/chats', search: '?id=chat-1' }, storage)).toBe(
      '/chats?id=chat-1'
    );
  });

  it('falls back to the saved path when login has no from', () => {
    storage.setItem(LAST_PATH_KEY, '/?id=chat-2');
    expect(pathAfterAuth(null, storage)).toBe('/?id=chat-2');
    expect(pathAfterAuth({ pathname: '/login' }, storage)).toBe('/?id=chat-2');
  });

  it('does not let a boot at / erase a restored chat', () => {
    storage.setItem(LAST_PATH_KEY, '/chats?id=chat-9');
    const history = {
      state: null,
      replaceState() {},
    };
    expect(lastPathToRestore('/', storage)).toBe('/chats?id=chat-9');
    restoreLastPath('/', history, storage);
    persistLastPath('/', storage);
    expect(storage.getItem(LAST_PATH_KEY)).toBe('/chats?id=chat-9');
    persistLastPath('/chats?id=chat-9', storage);
    persistLastPath('/', storage);
    expect(storage.getItem(LAST_PATH_KEY)).toBe('/');
  });
});
