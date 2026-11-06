import { describe, expect, it, mock } from 'bun:test';
import { frameType, publishFrame, subscribeFrames } from './frames';

describe('chat frame bus', () => {
  it("hands a frame to every subscriber of that chat, and only that chat's", () => {
    const first = mock();
    const second = mock();
    const elsewhere = mock();
    const stops = [
      subscribeFrames('chat-1', first),
      subscribeFrames('chat-1', second),
      subscribeFrames('chat-2', elsewhere),
    ];

    publishFrame('chat-1', { type: 'status', message: 'Thinking' });

    expect(first).toHaveBeenCalledWith({ type: 'status', message: 'Thinking' });
    expect(second).toHaveBeenCalledTimes(1);
    expect(elsewhere).not.toHaveBeenCalled();
    for (const stop of stops) stop();
  });

  it('stops handing frames to a listener once it unsubscribes', () => {
    const listener = mock();
    const stop = subscribeFrames('chat-1', listener);
    stop();

    publishFrame('chat-1', { type: 'init' });

    expect(listener).not.toHaveBeenCalled();
  });

  it('claims subagent frames for subscribers and leaves every other frame to the socket', () => {
    expect(publishFrame('chat-1', { type: 'subagent', chat_id: 'chat-1' })).toBe(true);
    expect(publishFrame('chat-1', { type: 'subagents_removed', ids: [] })).toBe(true);
    expect(publishFrame('chat-1', { type: 'init' })).toBe(false);
    expect(publishFrame('chat-1', { type: 'message_start', resumed: true })).toBe(false);
    expect(publishFrame('chat-1', { type: 'token', content: 'subagent' })).toBe(false);
    expect(publishFrame('chat-1', 'subagent')).toBe(false);
    expect(publishFrame('chat-1', null)).toBe(false);
    expect(publishFrame('chat-1', { type: ['subagent'] })).toBe(false);
  });

  it('reads the type of a frame, or nothing when it has no string type', () => {
    expect(frameType({ type: 'chunk', content: 'x' })).toBe('chunk');
    expect(frameType({ type: 7 })).toBeNull();
    expect(frameType({})).toBeNull();
    expect(frameType('subagent')).toBeNull();
    expect(frameType(null)).toBeNull();
  });

  it('claims a subagent frame even when nobody is listening yet', () => {
    expect(publishFrame('chat-unwatched', { type: 'subagent' })).toBe(true);
  });

  it('keeps handing frames to the rest when one subscriber throws', () => {
    const broken = mock(() => {
      throw new Error('broken');
    });
    const healthy = mock();
    const stops = [subscribeFrames('chat-1', broken), subscribeFrames('chat-1', healthy)];
    const microtasks = globalThis.queueMicrotask;
    const deferred: Array<() => void> = [];
    globalThis.queueMicrotask = (task) => deferred.push(task);

    try {
      expect(publishFrame('chat-1', { type: 'subagent' })).toBe(true);
    } finally {
      globalThis.queueMicrotask = microtasks;
      for (const stop of stops) stop();
    }

    expect(healthy).toHaveBeenCalledTimes(1);
    expect(deferred).toHaveLength(1);
    expect(() => deferred[0]()).toThrow('broken');
  });

  it('lets a listener unsubscribe while a frame is being handed out', () => {
    const later = mock();
    let stopFirst: () => void = () => undefined;
    const first = mock(() => stopFirst());
    stopFirst = subscribeFrames('chat-1', first);
    const stopLater = subscribeFrames('chat-1', later);

    publishFrame('chat-1', { type: 'init' });
    publishFrame('chat-1', { type: 'init' });

    expect(first).toHaveBeenCalledTimes(1);
    expect(later).toHaveBeenCalledTimes(2);
    stopLater();
  });
});
