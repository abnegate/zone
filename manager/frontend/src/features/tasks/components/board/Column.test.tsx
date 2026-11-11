import { describe, expect, it } from 'bun:test';
import { DndContext } from '@dnd-kit/core';
import { render, screen, within } from '@testing-library/react';
import { STAGE_IDS, STAGES, stageOf } from '../../status';
import { Column } from './Column';
import { Skeleton } from './Skeleton';
import { StatusIcon } from './StatusIcon';
import { View } from './View';

const review = stageOf('review');

function renderColumn(count: number) {
  return render(
    <DndContext>
      <Column stage={review} count={count} view={new View()}>
        {Array.from({ length: count }, (_, index) => `Card ${index + 1}`).map((title) => (
          <li key={title}>{title}</li>
        ))}
      </Column>
    </DndContext>
  );
}

describe('Column', () => {
  it('is a region named exactly by the stage label', () => {
    renderColumn(2);
    const region = screen.getByRole('region', { name: 'In review' });
    expect(region.dataset.stage).toBe('review');
    expect(within(region).getByRole('heading', { level: 2, name: 'In review' })).toBeTruthy();
  });

  it('shows the count and lists its cards', () => {
    renderColumn(2);
    const region = screen.getByRole('region', { name: 'In review' });
    expect(region.querySelector('.board-column-count')?.textContent).toBe('2');
    expect(
      within(region)
        .getAllByRole('listitem')
        .map((item) => item.textContent)
    ).toEqual(['Card 1', 'Card 2']);
  });

  it('shows the empty placeholder only when the count is zero', () => {
    const { unmount } = renderColumn(0);
    const empty = screen.getByRole('region', { name: 'In review' });
    expect(within(empty).getByText('No tasks')).toBeTruthy();
    expect(empty.querySelector('.board-column-count')?.textContent).toBe('0');
    unmount();

    renderColumn(1);
    expect(screen.queryByText('No tasks')).toBeNull();
  });

  it('holds its cards inside the droppable body', () => {
    renderColumn(1);
    const region = screen.getByRole('region', { name: 'In review' });
    const body = region.querySelector('.board-column-body');
    expect(body?.querySelector('ul.board-column-cards')).toBeTruthy();
    expect(body?.hasAttribute('data-over')).toBe(false);
  });

  it('heads the column with the stage icon', () => {
    renderColumn(1);
    const region = screen.getByRole('region', { name: 'In review' });
    const icon = region.querySelector('.board-column-header svg');
    expect(icon?.getAttribute('class')).toBe('status-icon status-icon-review');
  });
});

describe('StatusIcon', () => {
  it('draws a hidden, distinct glyph for every stage', () => {
    const glyphs = STAGE_IDS.map((stage) => {
      const { container, unmount } = render(<StatusIcon stage={stage} />);
      const icon = container.querySelector('svg');
      expect(icon?.getAttribute('aria-hidden')).toBe('true');
      expect(icon?.getAttribute('class')).toBe(`status-icon status-icon-${stage}`);
      const markup = icon?.innerHTML ?? '';
      unmount();
      return markup;
    });
    expect(new Set(glyphs).size).toBe(STAGE_IDS.length);
  });
});

describe('Skeleton', () => {
  it('renders seven hidden columns of placeholder cards', () => {
    const { container } = render(<Skeleton />);
    const board = container.querySelector('.board');
    expect(board?.getAttribute('aria-hidden')).toBe('true');
    const columns = [...(board?.querySelectorAll<HTMLElement>('.board-column') ?? [])];
    expect(columns.map((column) => column.dataset.stage)).toEqual(STAGES.map((stage) => stage.id));
    for (const column of columns) {
      expect(column.querySelector('.board-column-header .status-icon')).toBeTruthy();
      expect(column.querySelectorAll('.task-card.board-skeleton-card')).toHaveLength(2);
    }
    expect(screen.queryByRole('region')).toBeNull();
  });

  it('tells assistive technology the tasks are loading, outside the hidden board', () => {
    const { container } = render(<Skeleton />);
    const status = screen.getByRole('status');
    expect(status).toHaveTextContent('Loading tasks');
    expect(status.closest('[aria-hidden="true"]')).toBeNull();
    expect(container.querySelector('.board')?.contains(status)).toBe(false);
  });
});
