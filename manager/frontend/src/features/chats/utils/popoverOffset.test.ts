import { describe, expect, it } from 'bun:test';
import { popoverOffset, VIEWPORT_MARGIN } from './popoverOffset';

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
});
