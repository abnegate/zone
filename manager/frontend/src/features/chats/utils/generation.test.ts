import { afterEach, beforeEach, describe, expect, it, type Mock, spyOn } from 'bun:test';
import { clearChatCaches } from './cache';
import {
  beginGeneration,
  rememberedGeneration,
  resumeGeneration,
  settleGeneration,
} from './generation';

describe('generation clock', () => {
  let clock: Mock<() => number>;

  beforeEach(() => {
    clock = spyOn(Date, 'now').mockReturnValue(0);
  });

  afterEach(() => {
    clock.mockRestore();
    clearChatCaches();
  });

  /** Moves the wall clock the turns are timed by. */
  function at(reading: number): void {
    clock.mockReturnValue(reading);
  }

  it('remembers when a turn began, for a chat opened again later', () => {
    at(1000);
    expect(beginGeneration('chat-1')).toBe(1000);

    at(9000);
    expect(rememberedGeneration('chat-1')).toBe(1000);
  });

  it('keeps the start of the reply it is already counting', () => {
    at(1000);
    beginGeneration('chat-1');

    at(2000);
    expect(resumeGeneration('chat-1', 'reply-1')).toBe(1000);
    at(60_000);
    expect(resumeGeneration('chat-1', 'reply-1')).toBe(1000);
  });

  it('starts again for a later reply in the same chat', () => {
    at(1000);
    beginGeneration('chat-1');
    resumeGeneration('chat-1', 'reply-1');

    at(60_000);
    expect(resumeGeneration('chat-1', 'reply-2')).toBe(60_000);
    expect(rememberedGeneration('chat-1')).toBe(60_000);
  });

  it('never lets a send inherit the reading a previous turn left', () => {
    at(1000);
    beginGeneration('chat-1');
    resumeGeneration('chat-1', 'reply-1');

    at(60_000);
    expect(beginGeneration('chat-1')).toBe(60_000);
  });

  it('forgets a chat whose turn ended, and only that chat', () => {
    at(1000);
    beginGeneration('chat-1');
    beginGeneration('chat-2');

    settleGeneration('chat-1');

    expect(rememberedGeneration('chat-1')).toBeNull();
    expect(rememberedGeneration('chat-2')).toBe(1000);
  });

  it('counts every chat left running on its own start', () => {
    at(1000);
    beginGeneration('chat-1');
    at(5000);
    beginGeneration('chat-2');

    expect(rememberedGeneration('chat-1')).toBe(1000);
    expect(rememberedGeneration('chat-2')).toBe(5000);
  });

  it('is emptied with every other chat cache on logout', () => {
    at(1000);
    beginGeneration('chat-1');

    clearChatCaches();

    expect(rememberedGeneration('chat-1')).toBeNull();
  });
});
