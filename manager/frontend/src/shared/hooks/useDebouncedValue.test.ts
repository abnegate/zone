import { describe, expect, it } from 'bun:test';
import { act, renderHook } from '@testing-library/react';
import { useDebouncedValue } from './useDebouncedValue';

function wait(milliseconds: number) {
  return act(() => new Promise((resolve) => setTimeout(resolve, milliseconds)));
}

describe('useDebouncedValue', () => {
  it('starts with the value it was given', () => {
    const { result } = renderHook(() => useDebouncedValue('zone', 50));

    expect(result.current).toBe('zone');
  });

  it('holds the previous value until the delay passes', async () => {
    const { result, rerender } = renderHook(({ value }) => useDebouncedValue(value, 50), {
      initialProps: { value: '' },
    });

    rerender({ value: 'z' });
    await wait(10);
    expect(result.current).toBe('');

    await wait(60);
    expect(result.current).toBe('z');
  });

  it('settles only on the last of a quick run of changes', async () => {
    const seen: string[] = [];
    const { rerender } = renderHook(
      ({ value }) => {
        const debounced = useDebouncedValue(value, 50);
        if (seen.at(-1) !== debounced) {
          seen.push(debounced);
        }
        return debounced;
      },
      { initialProps: { value: '' } }
    );

    for (const value of ['z', 'zo', 'zon']) {
      rerender({ value });
      await wait(10);
    }
    await wait(80);

    expect(seen).toEqual(['', 'zon']);
  });
});
