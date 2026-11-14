import type { MouseEvent } from 'react';
import type { Gesture } from './types';

export const TOGGLE: Gesture = 'toggle';

export const RANGE: Gesture = 'range';

export function gestureOf(event: MouseEvent): Gesture | null {
  if (event.shiftKey) return RANGE;
  if (event.metaKey || event.ctrlKey) return TOGGLE;
  return null;
}

export function keepText(event: MouseEvent): void {
  if (event.shiftKey) event.preventDefault();
}
