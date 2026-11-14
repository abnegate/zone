import { describe, expect, it } from 'bun:test';
import { CACHED_CHATS, ChatCache, cacheEpoch, clearChatCaches } from './cache';

describe('ChatCache', () => {
  it('forgets the chat used longest ago once it holds more than its capacity', () => {
    const cache = new ChatCache<number>();
    for (let index = 0; index <= CACHED_CHATS; index += 1) cache.remember(String(index), index);

    expect(cache.has('0')).toBe(false);
    expect(cache.recall('1')).toBe(1);
    expect(cache.recall(String(CACHED_CHATS))).toBe(CACHED_CHATS);
  });

  it('keeps a chat recalled again over one left untouched', () => {
    const cache = new ChatCache<number>(2);
    cache.remember('a', 1);
    cache.remember('b', 2);
    cache.recall('a');
    cache.remember('c', 3);

    expect(cache.has('a')).toBe(true);
    expect(cache.has('b')).toBe(false);
  });

  it('peeks without counting as a use', () => {
    const cache = new ChatCache<number>(2);
    cache.remember('a', 1);
    cache.remember('b', 2);
    expect(cache.peek('a')).toBe(1);
    cache.remember('c', 3);

    expect(cache.has('a')).toBe(false);
  });

  it('changes a remembered chat in place and ignores one it does not hold', () => {
    const cache = new ChatCache<string>();
    cache.remember('a', 'old');
    cache.update('a', (value) => `${value} and new`);
    cache.update('missing', () => 'ignored');

    expect(cache.peek('a')).toBe('old and new');
    expect(cache.has('missing')).toBe(false);
  });

  it('clears every cache together and marks the clear', () => {
    const first = new ChatCache<number>();
    const second = new ChatCache<number>();
    first.remember('a', 1);
    second.remember('a', 2);
    const before = cacheEpoch();

    clearChatCaches();

    expect(first.has('a')).toBe(false);
    expect(second.has('a')).toBe(false);
    expect(cacheEpoch()).toBe(before + 1);
  });
});
