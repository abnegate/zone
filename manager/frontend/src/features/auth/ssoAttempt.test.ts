import { beforeEach, describe, expect, it } from 'bun:test';
import { clearAttempt, readAttempt, SSO_ATTEMPT_KEY, safePath, saveAttempt } from './ssoAttempt';

function memoryStorage(): Storage {
  const data = new Map<string, string>();
  return {
    get length() {
      return data.size;
    },
    clear: () => data.clear(),
    getItem: (key: string) => data.get(key) ?? null,
    key: (index: number) => [...data.keys()][index] ?? null,
    removeItem: (key: string) => {
      data.delete(key);
    },
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
  };
}

function refusingStorage(): Storage {
  const refuse = () => {
    throw new Error('SecurityError');
  };
  return {
    length: 0,
    clear: refuse,
    getItem: refuse,
    key: refuse,
    removeItem: refuse,
    setItem: refuse,
  };
}

describe('safePath', () => {
  it('keeps a same-origin path', () => {
    expect(safePath('/')).toBe('/');
    expect(safePath('/chats?id=chat-1#latest')).toBe('/chats?id=chat-1#latest');
    expect(safePath('/settings/server/sso')).toBe('/settings/server/sso');
  });

  it.each([
    ['protocol-relative', '//evil.test'],
    ['backslash protocol-relative', '/\\evil.test'],
    ['absolute', 'https://evil.test'],
    ['relative', 'evil'],
    ['javascript', 'javascript:alert(1)'],
    ['tab smuggled', '/\t/evil.test'],
    ['newline smuggled', '/\n/evil.test'],
    ['empty', ''],
  ])('falls back to / for a %s path', (_name, value) => {
    expect(safePath(value)).toBe('/');
  });

  it('falls back to / for a value that is not a string', () => {
    expect(safePath(undefined)).toBe('/');
    expect(safePath(null)).toBe('/');
    expect(safePath(42)).toBe('/');
    expect(safePath({ pathname: '/chats' })).toBe('/');
  });
});

describe('attempt storage', () => {
  let storage: Storage;

  beforeEach(() => {
    storage = memoryStorage();
  });

  it('saves under zone_sso_attempt and reads it back', () => {
    saveAttempt({ state: 'state-1', next: '/chats?id=chat-1' }, storage);

    expect(SSO_ATTEMPT_KEY).toBe('zone_sso_attempt');
    expect(JSON.parse(storage.getItem(SSO_ATTEMPT_KEY) ?? '')).toEqual({
      state: 'state-1',
      next: '/chats?id=chat-1',
    });
    expect(readAttempt(storage)).toEqual({ state: 'state-1', next: '/chats?id=chat-1' });
  });

  it('saves an unsafe next as /', () => {
    saveAttempt({ state: 'state-1', next: '//evil.test' }, storage);

    expect(readAttempt(storage)).toEqual({ state: 'state-1', next: '/' });
  });

  it('reads an unsafe next written by someone else as /', () => {
    storage.setItem(SSO_ATTEMPT_KEY, JSON.stringify({ state: 'state-1', next: '/\\evil.test' }));

    expect(readAttempt(storage)).toEqual({ state: 'state-1', next: '/' });
  });

  it.each([
    ['nothing saved', null],
    ['not JSON', '{'],
    ['not an object', '"state-1"'],
    ['no state', JSON.stringify({ next: '/' })],
    ['an empty state', JSON.stringify({ state: '', next: '/' })],
    ['a state that is not a string', JSON.stringify({ state: 7, next: '/' })],
  ])('reads %s as no attempt', (_name, saved) => {
    if (saved !== null) {
      storage.setItem(SSO_ATTEMPT_KEY, saved);
    }

    expect(readAttempt(storage)).toBeNull();
  });

  it('clears the attempt', () => {
    saveAttempt({ state: 'state-1', next: '/' }, storage);

    clearAttempt(storage);

    expect(storage.getItem(SSO_ATTEMPT_KEY)).toBeNull();
    expect(readAttempt(storage)).toBeNull();
  });

  it('reads no attempt and clears nothing when the browser refuses storage', () => {
    const refusing = refusingStorage();

    expect(readAttempt(refusing)).toBeNull();
    expect(() => clearAttempt(refusing)).not.toThrow();
    expect(() => saveAttempt({ state: 'state-1', next: '/' }, refusing)).toThrow();
  });
});
