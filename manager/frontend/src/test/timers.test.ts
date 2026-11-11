import { afterEach, describe, expect, it, vi } from 'bun:test';
import { fakeTimers } from './timers';

describe('fakeTimers', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('fires a zero-delay timeout although an earlier one was pending when it started', async () => {
    const fired: string[] = [];
    setTimeout(() => fired.push('earlier'), 0);

    await fakeTimers();
    setTimeout(() => fired.push('faked'), 0);
    vi.advanceTimersByTime(0);

    expect(fired).toEqual(['earlier', 'faked']);
  });

  it('holds a later timeout until its time has passed', async () => {
    const fired: string[] = [];

    await fakeTimers();
    setTimeout(() => fired.push('later'), 1000);
    vi.advanceTimersByTime(999);
    expect(fired).toEqual([]);

    vi.advanceTimersByTime(1);
    expect(fired).toEqual(['later']);
  });
});
