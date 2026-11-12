import { describe, expect, it } from 'bun:test';
import { Selection } from './Selection';

const COLUMN = ['task-1', 'task-2', 'task-3', 'task-4', 'task-5'];

function idsOf(selection: Selection): string[] {
  return [...selection.ids].sort();
}

describe('Selection', () => {
  it('starts empty without an anchor', () => {
    expect(Selection.EMPTY.size).toBe(0);
    expect(Selection.EMPTY.anchor).toBeNull();
    expect(new Selection().size).toBe(0);
  });

  it('collapses duplicate ids', () => {
    const selection = new Selection(['task-1', 'task-1', 'task-2']);

    expect(selection.size).toBe(2);
    expect(selection.has('task-1')).toBe(true);
    expect(selection.has('task-3')).toBe(false);
  });

  it('toggles a task on and anchors on it', () => {
    const selection = Selection.EMPTY.toggle('task-2');

    expect(idsOf(selection)).toEqual(['task-2']);
    expect(selection.anchor).toBe('task-2');
    expect(Selection.EMPTY.size).toBe(0);
  });

  it('toggles a selected task off and anchors on it', () => {
    const original = new Selection(['task-1', 'task-2'], 'task-1');
    const selection = original.toggle('task-2');

    expect(idsOf(selection)).toEqual(['task-1']);
    expect(selection.anchor).toBe('task-2');
    expect(idsOf(original)).toEqual(['task-1', 'task-2']);
    expect(original.anchor).toBe('task-1');
  });

  it('extends forward from the anchor through the column', () => {
    const original = Selection.EMPTY.toggle('task-2');
    const selection = original.extend('task-4', COLUMN);

    expect(idsOf(selection)).toEqual(['task-2', 'task-3', 'task-4']);
    expect(selection.anchor).toBe('task-2');
    expect(idsOf(original)).toEqual(['task-2']);
  });

  it('extends backward from the anchor through the column', () => {
    const selection = Selection.EMPTY.toggle('task-4').extend('task-1', COLUMN);

    expect(idsOf(selection)).toEqual(['task-1', 'task-2', 'task-3', 'task-4']);
    expect(selection.anchor).toBe('task-4');
  });

  it('keeps tasks already selected outside the range', () => {
    const selection = new Selection(['task-5', 'task-2'], 'task-2').extend('task-3', COLUMN);

    expect(idsOf(selection)).toEqual(['task-2', 'task-3', 'task-5']);
  });

  it('extends from the same anchor again after a range', () => {
    const selection = Selection.EMPTY.toggle('task-1')
      .extend('task-3', COLUMN)
      .extend('task-5', COLUMN);

    expect(idsOf(selection)).toEqual(['task-1', 'task-2', 'task-3', 'task-4', 'task-5']);
    expect(selection.anchor).toBe('task-1');
  });

  it('adds just the task and anchors on it without an anchor', () => {
    const selection = Selection.EMPTY.extend('task-3', COLUMN);

    expect(idsOf(selection)).toEqual(['task-3']);
    expect(selection.anchor).toBe('task-3');
  });

  it('adds just the task and anchors on it when the anchor is in another column', () => {
    const original = Selection.EMPTY.toggle('other-1');
    const selection = original.extend('task-3', COLUMN);

    expect(idsOf(selection)).toEqual(['other-1', 'task-3']);
    expect(selection.anchor).toBe('task-3');
    expect(original.anchor).toBe('other-1');
  });

  it('adds just the task when it is missing from the column', () => {
    const selection = Selection.EMPTY.toggle('task-1').extend('other-1', COLUMN);

    expect(idsOf(selection)).toEqual(['other-1', 'task-1']);
    expect(selection.anchor).toBe('other-1');
  });

  it('retains itself when every selected task is still listed', () => {
    const selection = new Selection(['task-1', 'task-2'], 'task-2');

    expect(selection.retain(COLUMN)).toBe(selection);
    expect(Selection.EMPTY.retain([])).toBe(Selection.EMPTY);
  });

  it('drops tasks that are no longer listed and keeps a listed anchor', () => {
    const original = new Selection(['task-1', 'task-2', 'task-3'], 'task-1');
    const selection = original.retain(new Set(['task-1', 'task-3']));

    expect(idsOf(selection)).toEqual(['task-1', 'task-3']);
    expect(selection.anchor).toBe('task-1');
    expect(idsOf(original)).toEqual(['task-1', 'task-2', 'task-3']);
  });

  it('clears an anchor that is no longer listed', () => {
    const original = new Selection(['task-1', 'task-2'], 'task-2');
    const selection = original.retain(['task-1']);

    expect(idsOf(selection)).toEqual(['task-1']);
    expect(selection.anchor).toBeNull();
    expect(original.anchor).toBe('task-2');
  });

  it('clears an unselected anchor that is no longer listed', () => {
    const selection = new Selection(['task-1'], 'task-2').retain(['task-1']);

    expect(idsOf(selection)).toEqual(['task-1']);
    expect(selection.anchor).toBeNull();
  });

  it('toggles on the retained selection without reviving dropped tasks', () => {
    const original = new Selection(['task-1', 'ghost'], 'ghost');
    const selection = original.retain(COLUMN).toggle('task-2');

    expect(idsOf(selection)).toEqual(['task-1', 'task-2']);
    expect(selection.anchor).toBe('task-2');
  });

  it('extends on the retained selection from a fresh anchor when the anchor dropped', () => {
    const selection = new Selection(['ghost'], 'ghost').retain(COLUMN).extend('task-3', COLUMN);

    expect(idsOf(selection)).toEqual(['task-3']);
    expect(selection.anchor).toBe('task-3');
  });

  it('clears to the empty selection', () => {
    const original = new Selection(['task-1', 'task-2'], 'task-1');

    expect(original.clear()).toBe(Selection.EMPTY);
    expect(idsOf(original)).toEqual(['task-1', 'task-2']);
  });

  it('does not let callers change the selected ids from outside', () => {
    const source = ['task-1'];
    const selection = new Selection(source);
    source.push('task-2');

    expect(idsOf(selection)).toEqual(['task-1']);
  });
});
