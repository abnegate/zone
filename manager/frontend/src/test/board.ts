import { spyOn } from 'bun:test';
import { act, fireEvent } from '@testing-library/react';

const COLUMN_WIDTH = 280;
const COLUMN_STEP = 300;
const COLUMN_HEIGHT = 600;
const CARD_INSET = 10;
const CARD_HEIGHT = 80;
const CARD_STEP = 90;
const HEADER_HEIGHT = 60;

function box(left: number, top: number, width: number, height: number): DOMRect {
  return {
    x: left,
    y: top,
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
    toJSON: () => undefined,
  } as DOMRect;
}

export function layOutBoard(): () => void {
  const original = HTMLElement.prototype.getBoundingClientRect;
  const measure = spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(
    function (this: HTMLElement) {
      const overlaid =
        this.closest('.task-card-overlay') ?? this.querySelector('.task-card-overlay');
      const item = overlaid
        ? document.querySelector<HTMLElement>('.board .task-card-dragging')
        : this.closest<HTMLElement>('.board .task-card');
      const section = (item ?? this).closest<HTMLElement>('[data-stage]');
      if (!section?.parentElement) return original.call(this);
      const left =
        Array.from(section.parentElement.querySelectorAll('[data-stage]')).indexOf(section) *
        COLUMN_STEP;
      if (!item) return box(left, 0, COLUMN_WIDTH, COLUMN_HEIGHT);
      const index = Array.from(section.querySelectorAll('.task-card')).indexOf(item);
      return box(
        left + CARD_INSET,
        HEADER_HEIGHT + index * CARD_STEP,
        COLUMN_WIDTH - 2 * CARD_INSET,
        CARD_HEIGHT
      );
    }
  );
  return () => measure.mockRestore();
}

export async function pressKey(target: HTMLElement, code: string): Promise<void> {
  await act(async () => {
    fireEvent.keyDown(target, { code, key: code === 'Space' ? ' ' : code });
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}
