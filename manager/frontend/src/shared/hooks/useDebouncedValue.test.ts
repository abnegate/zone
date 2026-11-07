import { beforeEach, describe, expect, it, vi } from 'bun:test';
import { act, renderHook } from '@testing-library/react';
import { useDebouncedValue } from './useDebouncedValue';

const DELAY = 50;

function advance(milliseconds: number) {
  act(() => {
    vi.advanceTimersByTime(milliseconds);
  });
}

describe('useDebouncedValue', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  it('starts with the value it was given', () => {
    const { result } = renderHook(() => useDebouncedValue('zone', DELAY));

    expect(result.current).toBe('zone');
  });

  it('holds the previous value until the delay passes', () => {
    const { result, rerender } = renderHook(({ value }) => useDebouncedValue(value, DELAY), {
      initialProps: { value: '' },
    });

    rerender({ value: 'z' });
    advance(DELAY - 1);
    expect(result.current).toBe('');

    advance(1);
    expect(result.current).toBe('z');
  });

  it('settles only on the last of a quick run of changes', () => {
    const seen: string[] = [];
    const { rerender } = renderHook(
      ({ value }) => {
        const debounced = useDebouncedValue(value, DELAY);
        if (seen.at(-1) !== debounced) {
          seen.push(debounced);
        }
        return debounced;
      },
      { initialProps: { value: '' } }
    );

    for (const value of ['z', 'zo', 'zon']) {
      rerender({ value });
      advance(DELAY - 1);
    }
    expect(seen).toEqual(['']);

    advance(1);
    expect(seen).toEqual(['', 'zon']);
  });
});
