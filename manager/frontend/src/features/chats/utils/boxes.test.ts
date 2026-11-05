import { describe, expect, it } from 'bun:test';
import {
  boxTitle,
  DEFAULT_ORDER,
  dropSide,
  isPanelBox,
  mergeOrder,
  moveBox,
  PANEL_BOXES,
  parseOrder,
} from './boxes';

describe('panel boxes', () => {
  it('lists every box once, by a stable id', () => {
    expect(DEFAULT_ORDER).toEqual(['changes', 'subagents', 'tasks', 'sources', 'gallery']);
    expect(new Set(PANEL_BOXES.map((box) => box.id)).size).toBe(PANEL_BOXES.length);
    expect(boxTitle('gallery')).toBe('Gallery');
    expect(boxTitle('subagents')).toBe('Agents');
    expect(isPanelBox('sources')).toBe(true);
    expect(isPanelBox('tasks')).toBe(true);
    expect(isPanelBox('subagents')).toBe(true);
    expect(isPanelBox('retired')).toBe(false);
    expect(isPanelBox(3)).toBe(false);
  });

  it('reads a saved order, and nothing from one it cannot read', () => {
    expect(parseOrder('["gallery","changes"]')).toEqual(['gallery', 'changes']);
    expect(parseOrder('["gallery",4,null,"sources"]')).toEqual(['gallery', 'sources']);
    for (const stored of [null, '', 'gallery', '{"gallery":1}', '"gallery"', '[']) {
      expect(parseOrder(stored)).toEqual([]);
    }
  });

  it('keeps the default order for a viewer who never moved a box', () => {
    expect(mergeOrder([])).toEqual([...DEFAULT_ORDER]);
  });

  it("keeps a viewer's order", () => {
    expect(mergeOrder(['gallery', 'sources', 'subagents', 'tasks', 'changes'])).toEqual([
      'gallery',
      'sources',
      'subagents',
      'tasks',
      'changes',
    ]);
  });

  it('puts a box the viewer never placed after the ones they did, in its default order', () => {
    expect(mergeOrder(['gallery'])).toEqual([
      'gallery',
      'changes',
      'subagents',
      'tasks',
      'sources',
    ]);
    expect(mergeOrder(['gallery', 'sources', 'changes'])).toEqual([
      'gallery',
      'sources',
      'changes',
      'subagents',
      'tasks',
    ]);
    expect(mergeOrder(['sources'], ['changes', 'sources', 'gallery'])).toEqual([
      'sources',
      'changes',
      'gallery',
    ]);
  });

  it('puts the agents last for a viewer who saved an order before the box existed', () => {
    expect(mergeOrder(['changes', 'tasks', 'sources', 'gallery'])).toEqual([
      'changes',
      'tasks',
      'sources',
      'gallery',
      'subagents',
    ]);
  });

  it('drops a saved box that no longer exists, and a box saved twice', () => {
    expect(mergeOrder(['archive', 'gallery', 'retired', 'gallery', 'changes'])).toEqual([
      'gallery',
      'changes',
      'subagents',
      'tasks',
      'sources',
    ]);
    expect(mergeOrder(['gallery', 'changes'], ['changes', 'sources'])).toEqual([
      'changes',
      'sources',
    ]);
  });

  it('moves a box to a new place among the boxes shown', () => {
    const order = ['changes', 'sources', 'gallery'] as const;

    expect(moveBox(order, order, 'gallery', 0)).toEqual(['gallery', 'changes', 'sources']);
    expect(moveBox(order, order, 'changes', 2)).toEqual(['sources', 'gallery', 'changes']);
    expect(moveBox(order, order, 'sources', 1)).toEqual(['changes', 'sources', 'gallery']);
  });

  it('keeps a box with nothing to show where it was, while the others move around it', () => {
    const order = ['changes', 'sources', 'gallery'] as const;

    expect(moveBox(order, ['changes', 'gallery'], 'gallery', 0)).toEqual([
      'gallery',
      'sources',
      'changes',
    ]);
  });

  it('keeps a move inside the list, and ignores a box that is not shown', () => {
    const order = ['changes', 'sources', 'gallery'] as const;

    expect(moveBox(order, order, 'changes', 9)).toEqual(['sources', 'gallery', 'changes']);
    expect(moveBox(order, order, 'gallery', -4)).toEqual(['gallery', 'changes', 'sources']);
    expect(moveBox(order, ['changes', 'sources'], 'gallery', 0)).toEqual([...order]);
  });

  it('marks the side of the box being dragged over that the dragged box would land on', () => {
    const shown = ['changes', 'sources', 'gallery'] as const;

    expect(dropSide(shown, 'gallery', 'changes', 'changes')).toBe('before');
    expect(dropSide(shown, 'changes', 'gallery', 'gallery')).toBe('after');
    expect(dropSide(shown, 'changes', 'gallery', 'sources')).toBeNull();
    expect(dropSide(shown, 'changes', 'changes', 'changes')).toBeNull();
    expect(dropSide(shown, null, 'changes', 'changes')).toBeNull();
    expect(dropSide(shown, 'changes', null, 'changes')).toBeNull();
  });
});
