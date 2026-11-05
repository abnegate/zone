import { describe, expect, it } from 'bun:test';
import { popoverOffset, VIEWPORT_MARGIN, viewportMargin } from './popoverOffset';

describe('popoverOffset', () => {
  it('leaves a panel that fits under its trigger aligned to the trigger', () => {
    expect(popoverOffset({ left: 500, right: 700 }, 320, 1280)).toBe(0);
  });

  it('flips a panel that would run off the right to end at the trigger', () => {
    expect(popoverOffset({ left: 1000, right: 1260 }, 320, 1280)).toBe(-60);
  });

  it('keeps a flipped panel inside the right margin when the trigger is narrower', () => {
    const offset = popoverOffset({ left: 1200, right: 1270 }, 320, 1280);
    expect(1200 + offset + 320).toBe(1280 - VIEWPORT_MARGIN);
  });

  it('keeps a panel on a phone inside both margins', () => {
    const trigger = { left: 200, right: 300 };
    const offset = popoverOffset(trigger, 343, 375);
    expect(trigger.left + offset).toBe(VIEWPORT_MARGIN);
  });

  it('never starts a panel past the left margin, even wider than the screen', () => {
    const trigger = { left: 30, right: 120 };
    expect(trigger.left + popoverOffset(trigger, 400, 375)).toBe(VIEWPORT_MARGIN);
  });

  it('keeps a clamped panel inside the margin it is given', () => {
    const offset = popoverOffset({ left: 1200, right: 1270 }, 320, 1280, 20);
    expect(1200 + offset + 320).toBe(1260);
  });
});

describe('viewportMargin', () => {
  it('falls back to the default margin without a gutter token', () => {
    expect(viewportMargin(document.createElement('div'))).toBe(VIEWPORT_MARGIN);
  });

  it('resolves a rem gutter against the root font size', () => {
    const element = document.createElement('div');
    element.style.setProperty('--ui-gutter', '1.25rem');
    document.body.append(element);
    const root = document.documentElement.style.fontSize;
    document.documentElement.style.fontSize = '16px';
    try {
      expect(viewportMargin(element)).toBe(20);
    } finally {
      document.documentElement.style.fontSize = root;
      element.remove();
    }
  });

  it('reads a pixel gutter as is', () => {
    const element = document.createElement('div');
    element.style.setProperty('--ui-gutter', '12px');
    document.body.append(element);
    try {
      expect(viewportMargin(element)).toBe(12);
    } finally {
      element.remove();
    }
  });
});
