import { afterEach, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useMemo, useState } from 'react';
import { STAGES } from '../../status';
import type { BoardActions, Task, TaskStatus } from '../../types';
import { Board } from './Board';

const COLUMN_WIDTH = 280;
const COLUMN_STEP = 300;
const COLUMN_HEIGHT = 600;
const CARD_INSET = 10;
const CARD_HEIGHT = 80;
const CARD_STEP = 90;
const HEADER_HEIGHT = 60;
const SERVER_DELAY_MS = 50;

const PROJECTS = new Map([
  ['zone', 'Zone'],
  ['cloud', 'Cloud'],
]);

function task(id: string, title: string, status: TaskStatus, project_ids: string[]): Task {
  return {
    id,
    workspace_id: 'workspace-1',
    project_ids,
    title,
    description: '',
    acceptance_criteria: null,
    status,
    priority: null,
    model_name: null,
    dependencies: [],
    created_at: null,
    updated_at: null,
    started_at: null,
    completed_at: null,
    is_agentic: false,
    require_plan_approval: false,
    github_repo_url: null,
    source_id: null,
    source_ids: [],
    queued_at: null,
    worker_id: null,
    pr_url: null,
    branch_name: null,
    pr_status: null,
    pr_created_at: null,
  };
}

const TASKS: Task[] = [
  task('write', 'Write the spec', 'created', ['zone', 'cloud']),
  task('queue', 'Queue the build', 'queued', []),
  task('build', 'Build the board', 'in_progress', ['zone', 'gone']),
  task('ship', 'Ship it', 'review', ['cloud']),
  task('stop', 'Stop the old table', 'cancelled', ['zone']),
];

function actions(): BoardActions {
  return {
    open: mock(),
    run: mock(),
    rename: mock(),
    retitle: mock(() => Promise.resolve()),
    finish: mock(),
    remove: mock(),
    move: mock(() => Promise.resolve()),
  };
}

function renderBoard(handlers: BoardActions = actions()) {
  render(<Board tasks={TASKS} projects={PROJECTS} renaming={null} actions={handlers} />);
  return handlers;
}

function pause() {
  return new Promise((resolve) => setTimeout(resolve, SERVER_DELAY_MS));
}

function MovingBoard({ refuse = false }: { refuse?: boolean }) {
  const [tasks, setTasks] = useState(TASKS);
  const handlers = useMemo<BoardActions>(() => {
    const place = (id: string, status: TaskStatus) =>
      setTasks((list) => list.map((item) => (item.id === id ? { ...item, status } : item)));
    return {
      ...actions(),
      move: async (moved, status) => {
        await pause();
        place(moved.id, status);
        await pause();
        if (refuse) place(moved.id, moved.status);
      },
    };
  }, [refuse]);
  return <Board tasks={tasks} projects={PROJECTS} renaming={null} actions={handlers} />;
}

function board() {
  return screen.getByRole('region', { name: 'Tasks' });
}

function columns() {
  return Array.from(board().querySelectorAll<HTMLElement>('.board-column'));
}

function column(name: string) {
  return within(board()).getByRole('region', { name });
}

function card(title: string) {
  const open = screen.getByRole('button', { name: title });
  const item = open.closest<HTMLElement>('.task-card');
  if (!item) throw new Error(`No card for ${title}`);
  return item;
}

describe('Board', () => {
  it('shows the seven columns in order with their counts', () => {
    renderBoard();

    expect(columns().map((section) => section.getAttribute('data-stage'))).toEqual(
      STAGES.map((stage) => stage.id)
    );
    expect(columns().map((section) => within(section).getByRole('heading').textContent)).toEqual([
      'Planned',
      'In progress',
      'Blocked',
      'Paused',
      'In review',
      'Completed',
      'Cancelled',
    ]);
    expect(
      columns().map((section) => section.querySelector('.board-column-count')?.textContent)
    ).toEqual(['2', '1', '0', '0', '1', '0', '1']);
  });

  it('puts each card in the column for its status, keeping server order', () => {
    renderBoard();

    const planned = within(column('Planned'));
    expect(
      planned
        .getAllByRole('button', { name: /^(Write the spec|Queue the build)$/ })
        .map((button) => button.textContent)
    ).toEqual(['Write the spec', 'Queue the build']);
    expect(within(column('In progress')).getByText('Build the board')).toBeInTheDocument();
    expect(within(column('In review')).getByText('Ship it')).toBeInTheDocument();
    expect(within(column('Cancelled')).getByText('Stop the old table')).toBeInTheDocument();
  });

  it('names each card its projects, with fallbacks for none and for an unknown one', () => {
    renderBoard();

    const project = (title: string) => card(title).querySelector('.task-card-project')?.textContent;
    expect(project('Write the spec')).toBe('Zone, Cloud');
    expect(project('Queue the build')).toBe('No projects');
    expect(project('Build the board')).toBe('Zone, Unknown');
    expect(project('Ship it')).toBe('Cloud');
  });

  it('tells a keyboard user how to move a card', () => {
    renderBoard();

    const handle = screen.getByRole('button', { name: 'Move Write the spec' });
    expect(
      document.getElementById(handle.getAttribute('aria-describedby') ?? '')
    ).toHaveTextContent(/press space to pick it up, the left and right arrow keys/);
  });

  describe('dragging', () => {
    let measure: ReturnType<typeof spyOn>;

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

    beforeEach(() => {
      const original = HTMLElement.prototype.getBoundingClientRect;
      measure = spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
        this: HTMLElement
      ) {
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
      });
    });

    afterEach(() => {
      measure.mockRestore();
    });

    function grip(title: string): HTMLElement {
      return screen.getByRole('button', { name: `Move ${title}` });
    }

    async function press(target: HTMLElement, code: string) {
      await act(async () => {
        fireEvent.keyDown(target, { code, key: code === 'Space' ? ' ' : code });
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    }

    function announced(text: string) {
      return waitFor(() => expect(screen.getByText(text)).toBeInTheDocument());
    }

    it('moves a card to the next column by keyboard and says so', async () => {
      const handlers = renderBoard();
      const handle = grip('Write the spec');
      handle.focus();

      await press(handle, 'Space');
      await announced('Picked up Write the spec, in Planned.');
      await press(handle, 'ArrowRight');
      await announced('Write the spec is over In progress.');
      await press(handle, 'Space');
      await announced('Write the spec moved to In progress.');

      expect(handlers.move).toHaveBeenCalledTimes(1);
      expect(handlers.move).toHaveBeenCalledWith(TASKS[0], 'in_progress');
    });

    it('moves a card back to Planned as created', async () => {
      const handlers = renderBoard();
      const handle = grip('Ship it');
      handle.focus();

      await press(handle, 'Space');
      await announced('Picked up Ship it, in In review.');
      for (let step = 0; step < 4; step += 1) await press(handle, 'ArrowLeft');
      await announced('Ship it is over Planned.');
      await press(handle, 'Enter');
      await announced('Ship it moved to Planned.');

      expect(handlers.move).toHaveBeenCalledWith(TASKS[3], 'created');
    });

    it('leaves a card dropped on its own column alone', async () => {
      const handlers = renderBoard();
      const handle = grip('Queue the build');
      handle.focus();

      await press(handle, 'Space');
      await announced('Picked up Queue the build, in Planned.');
      await press(handle, 'Space');
      await announced('Queue the build stays in Planned.');

      expect(handlers.move).not.toHaveBeenCalled();
    });

    it('stays put past the last column', async () => {
      const handlers = renderBoard();
      const handle = grip('Stop the old table');
      handle.focus();

      await press(handle, 'Space');
      await announced('Picked up Stop the old table, in Cancelled.');
      await press(handle, 'ArrowRight');
      await press(handle, 'Space');
      await announced('Stop the old table stays in Cancelled.');

      expect(handlers.move).not.toHaveBeenCalled();
    });

    async function carryRight(title: string) {
      const handle = grip(title);
      handle.focus();
      await press(handle, 'Space');
      await announced(`Picked up ${title}, in Planned.`);
      await press(handle, 'ArrowRight');
      await announced(`${title} is over In progress.`);
      await press(handle, 'Space');
      await announced(`${title} moved to In progress.`);
    }

    it('keeps focus on the grip of a card that moved to another column', async () => {
      render(<MovingBoard />);

      await carryRight('Write the spec');

      await waitFor(() => {
        expect(within(column('In progress')).getByText('Write the spec')).toBeInTheDocument();
      });
      await waitFor(() => expect(document.activeElement).toBe(grip('Write the spec')));
    });

    it('keeps focus on the grip of a card whose move was refused', async () => {
      render(<MovingBoard refuse />);

      await carryRight('Write the spec');

      await waitFor(() => {
        expect(within(column('In progress')).getByText('Write the spec')).toBeInTheDocument();
      });
      await waitFor(() => expect(document.activeElement).toBe(grip('Write the spec')));
      await waitFor(() => {
        expect(within(column('Planned')).getByText('Write the spec')).toBeInTheDocument();
      });
      await waitFor(() => expect(document.activeElement).toBe(grip('Write the spec')));
    });

    it('puts a card back when the move is cancelled', async () => {
      const handlers = renderBoard();
      const handle = grip('Build the board');
      handle.focus();

      await press(handle, 'Space');
      await announced('Picked up Build the board, in In progress.');
      await press(handle, 'ArrowRight');
      await announced('Build the board is over Blocked.');
      await press(handle, 'Escape');
      await announced('Moving Build the board cancelled. It stays in In progress.');

      expect(handlers.move).not.toHaveBeenCalled();
    });
  });
});
