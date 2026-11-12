import { describe, expect, it, mock } from 'bun:test';
import type { MouseEvent } from 'react';
import { gestureOf, keepText, RANGE, TOGGLE } from './gesture';

interface Keys {
  shiftKey?: boolean;
  metaKey?: boolean;
  ctrlKey?: boolean;
}

function click({ shiftKey = false, metaKey = false, ctrlKey = false }: Keys = {}) {
  const preventDefault = mock();
  const event = { shiftKey, metaKey, ctrlKey, preventDefault } as unknown as MouseEvent;
  return { event, preventDefault };
}

describe('gesture', () => {
  it('names the two selection gestures', () => {
    expect(TOGGLE).toBe('toggle');
    expect(RANGE).toBe('range');
  });

  it('reads no gesture from a plain click', () => {
    expect(gestureOf(click().event)).toBeNull();
  });

  it('reads a toggle from a command or control click', () => {
    expect(gestureOf(click({ metaKey: true }).event)).toBe(TOGGLE);
    expect(gestureOf(click({ ctrlKey: true }).event)).toBe(TOGGLE);
  });

  it('reads a range from a shift click, even with command held', () => {
    expect(gestureOf(click({ shiftKey: true }).event)).toBe(RANGE);
    expect(gestureOf(click({ shiftKey: true, metaKey: true }).event)).toBe(RANGE);
  });

  it('keeps a shift press from selecting text and leaves other presses alone', () => {
    const shifted = click({ shiftKey: true });
    keepText(shifted.event);
    expect(shifted.preventDefault).toHaveBeenCalledTimes(1);

    const plain = click({ metaKey: true });
    keepText(plain.event);
    expect(plain.preventDefault).not.toHaveBeenCalled();
  });
});
