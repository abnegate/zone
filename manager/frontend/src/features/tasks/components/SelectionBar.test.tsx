import { describe, expect, it, mock } from 'bun:test';
import { join } from 'node:path';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { media, read, rule } from '../../../test/css';
import { SelectionBar } from './SelectionBar';

const css = read(join(import.meta.dir, 'SelectionBar.css'));

function renderBar(count = 3) {
  const onEdit = mock();
  const onClear = mock();
  render(<SelectionBar count={count} onEdit={onEdit} onClear={onClear} />);
  return { onEdit, onClear, toolbar: screen.getByRole('toolbar', { name: 'Selected tasks' }) };
}

describe('SelectionBar', () => {
  it('names the toolbar for the selected tasks', () => {
    const { toolbar } = renderBar();
    expect(toolbar.className).toContain('tasks-selection');
  });

  it('announces how many tasks are selected', () => {
    const { toolbar } = renderBar(4);
    const status = within(toolbar).getByText('4 selected');
    expect(status.getAttribute('aria-live')).toBe('polite');
  });

  it('keeps the announcement current as the count changes', () => {
    const { rerender } = render(<SelectionBar count={2} onEdit={() => {}} onClear={() => {}} />);
    const status = screen.getByText('2 selected');
    rerender(<SelectionBar count={5} onEdit={() => {}} onClear={() => {}} />);
    expect(status.textContent).toBe('5 selected');
    expect(screen.queryByText('2 selected')).toBeNull();
  });

  it('edits the selected tasks', () => {
    const { onEdit, onClear, toolbar } = renderBar();
    fireEvent.click(within(toolbar).getByRole('button', { name: 'Edit' }));
    expect(onEdit).toHaveBeenCalledTimes(1);
    expect(onClear).not.toHaveBeenCalled();
  });

  it('clears the selection', () => {
    const { onEdit, onClear, toolbar } = renderBar();
    fireEvent.click(within(toolbar).getByRole('button', { name: 'Clear' }));
    expect(onClear).toHaveBeenCalledTimes(1);
    expect(onEdit).not.toHaveBeenCalled();
  });

  it('offers editing first and clearing as the quieter action', () => {
    const { toolbar } = renderBar();
    const buttons = within(toolbar).getAllByRole('button');
    expect(buttons.map((button) => button.textContent)).toEqual(['Edit', 'Clear']);
    expect(buttons[0].className).toContain('ui-btn-primary');
    expect(buttons[1].className).toContain('ui-btn-ghost');
    expect(buttons.every((button) => button.getAttribute('type') === 'button')).toBe(true);
  });
});

describe('SelectionBar layout', () => {
  const bar = rule(css, '.tasks-selection');

  it('floats at the bottom centre above the board and below dialogs', () => {
    expect(bar).toContain('position: fixed');
    expect(bar).toContain('margin-inline: auto');
    expect(bar).toContain('width: fit-content');
    expect(bar).toContain('z-index: calc(var(--ui-layer-modal) - 1)');
  });

  it('keeps a 16px gutter clear of the safe area so a phone never scrolls sideways', () => {
    expect(bar).toContain('right: calc(var(--ui-space-4) + var(--ui-safe-right))');
    expect(bar).toContain('bottom: calc(var(--ui-space-4) + var(--ui-safe-bottom))');
    const phone = media(css, '(max-width: 768px)');
    expect(
      rule(phone, ".tasks-selection, :root[data-sidebar-collapsed='true'] .tasks-selection")
    ).toContain('left: calc(var(--ui-space-4) + var(--ui-safe-left))');
  });

  it('centres over the content beside the sidebar on wide screens', () => {
    expect(bar).toContain('left: calc(var(--ui-sidebar-width) + var(--ui-space-4))');
    expect(rule(css, ":root[data-sidebar-collapsed='true'] .tasks-selection")).toContain(
      'left: calc(var(--ui-sidebar-collapsed) + var(--ui-space-4))'
    );
  });

  it('truncates the count rather than widening the bar', () => {
    const count = rule(css, '.tasks-selection-count');
    expect(count).toContain('min-width: 0');
    expect(count).toContain('text-overflow: ellipsis');
    expect(count).toContain('white-space: nowrap');
  });

  it('lifts off the dark board with a brighter surface and a stronger edge', () => {
    const dark = rule(css, ":root[data-theme='dark'] .tasks-selection");
    expect(dark).toContain('background: var(--ui-bg-surface)');
    expect(dark).toContain('border-color: var(--ui-border-strong)');
  });

  it('appears without motion when reduced motion is asked for', () => {
    expect(bar).toContain('animation:');
    const reduced = media(css, '(prefers-reduced-motion: reduce)');
    expect(rule(reduced, '.tasks-selection')).toContain('animation: none');
  });
});
