import { afterEach, beforeEach, describe, expect, it, mock, setSystemTime, spyOn } from 'bun:test';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useMemo, useState } from 'react';
import { layOutBoard, pressKey } from '../../../../test/board';
import { STAGES } from '../../status';
import type { BoardActions, Task, TaskStatus } from '../../types';
import { Board } from './Board';
import { View } from './View';

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
    move: mock(() => Promise.resolve(true)),
  };
}

function renderBoard(handlers: BoardActions = actions()) {
  render(
    <Board
      tasks={TASKS}
      projects={PROJECTS}
      renaming={null}
      removed={null}
      view={new View()}
      actions={handlers}
    />
  );
  return handlers;
}

function MovingBoard({ answer }: { answer: Promise<boolean> }) {
  const [tasks, setTasks] = useState(TASKS);
  const [view] = useState(() => new View());
  const handlers = useMemo<BoardActions>(() => {
    const place = (id: string, status: TaskStatus) =>
      setTasks((list) => list.map((item) => (item.id === id ? { ...item, status } : item)));
    return {
      ...actions(),
      move: async (moved, status) => {
        place(moved.id, status);
        const accepted = await answer;
        if (!accepted) place(moved.id, moved.status);
        return accepted;
      },
    };
  }, [answer]);
  return (
    <Board
      tasks={tasks}
      projects={PROJECTS}
      renaming={null}
      removed={null}
      view={view}
      actions={handlers}
    />
  );
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

  it('names each card its projects, with a fallback for an unknown one and nothing for none', () => {
    renderBoard();

    const project = (title: string) => card(title).querySelector('.task-card-project')?.textContent;
    expect(project('Write the spec')).toBe('Zone, Cloud');
    expect(project('Queue the build')).toBeUndefined();
    expect(project('Build the board')).toBe('Zone, Unknown');
    expect(project('Ship it')).toBe('Cloud');
    expect(within(board()).queryByText('No projects')).not.toBeInTheDocument();
  });

  describe('related tasks', () => {
    const RELATED: Task[] = [
      TASKS[0],
      { ...TASKS[1], dependencies: ['write'] },
      { ...TASKS[2], dependencies: ['write', 'queue', 'gone'] },
      { ...TASKS[3], dependencies: ['build', 'write', 'queue', 'stop', 'deleted', 'gone'] },
      TASKS[4],
    ];

    function renderRelated(handlers: BoardActions = actions()) {
      render(
        <Board
          tasks={RELATED}
          projects={PROJECTS}
          renaming={null}
          removed={null}
          view={new View()}
          actions={handlers}
        />
      );
      return handlers;
    }

    function related(title: string, label: string): string[] {
      const list = within(card(title)).queryByRole('list', { name: label });
      if (!list) return [];
      return within(list)
        .getAllByRole('listitem')
        .map((item) => item.textContent ?? '');
    }

    it('names on each card one loaded task it depends on, with its status, and counts the rest', () => {
      renderRelated();

      expect(related('Queue the build', 'Depends on')).toEqual(['Write the specCreated']);
      expect(related('Build the board', 'Depends on')).toEqual([
        'Write the specCreated',
        '+2 more',
      ]);
      expect(related('Write the spec', 'Depends on')).toEqual([]);
    });

    it('names on each card one loaded task that depends on it and counts the rest', () => {
      renderRelated();

      expect(related('Write the spec', 'Blocks')).toEqual(['Queue the buildQueued', '+2 more']);
      expect(related('Queue the build', 'Blocks')).toEqual([
        'Build the boardIn progress',
        '+1 more',
      ]);
      expect(related('Stop the old table', 'Blocks')).toEqual(['Ship itIn review']);
      expect(related('Ship it', 'Blocks')).toEqual([]);
    });

    it('names one dependency, then counts the rest and the unknown together', () => {
      renderRelated();

      expect(related('Ship it', 'Depends on')).toEqual(['Build the boardIn progress', '+5 more']);
    });

    it('opens the related task from another card, not the card it is listed on', () => {
      const handlers = renderRelated();

      const host = within(card('Build the board'));
      fireEvent.click(host.getByRole('button', { name: 'Open Write the spec' }));

      expect(handlers.open).toHaveBeenCalledTimes(1);
      expect(handlers.open).toHaveBeenCalledWith(RELATED[0], RELATED[2]);
    });

    it('shows no relation on a board whose tasks have none', () => {
      renderBoard();

      expect(within(board()).queryByText('Depends on')).not.toBeInTheDocument();
      expect(within(board()).queryByText('Blocks')).not.toBeInTheDocument();
      expect(within(board()).queryByRole('button', { name: /^Open / })).not.toBeInTheDocument();
    });

    it('still moves a card that lists related tasks by keyboard from its grip', async () => {
      const restore = layOutBoard();
      const handlers = renderRelated();
      const handle = screen.getByRole('button', { name: 'Move Build the board' });
      handle.focus();

      await pressKey(handle, 'Space');
      await screen.findByText('Picked up Build the board, in In progress.');
      await pressKey(handle, 'ArrowRight');
      await screen.findByText('Build the board is over Blocked.');
      await pressKey(handle, 'Space');
      await screen.findByText('Build the board moved to Blocked.');
      restore();

      expect(handlers.move).toHaveBeenCalledTimes(1);
      expect(handlers.move).toHaveBeenCalledWith(RELATED[2], 'blocked');
      expect(handlers.open).not.toHaveBeenCalled();
    });

    it('does not pick a card up with Space or Enter on a related task', async () => {
      const restore = layOutBoard();
      const handlers = renderRelated();
      const open = within(card('Build the board')).getByRole('button', {
        name: 'Open Write the spec',
      });
      open.focus();

      await pressKey(open, 'Space');
      await pressKey(open, 'Enter');
      restore();

      expect(board()).not.toHaveAttribute('data-dragging');
      expect(handlers.move).not.toHaveBeenCalled();
    });
  });

  it('tells a keyboard user how to move a card', () => {
    renderBoard();

    const handle = screen.getByRole('button', { name: 'Move Write the spec' });
    expect(
      document.getElementById(handle.getAttribute('aria-describedby') ?? '')
    ).toHaveTextContent(/press space to pick it up, the left and right arrow keys/);
  });

  describe('dragging', () => {
    let restore: () => void = () => {};

    beforeEach(() => {
      restore = layOutBoard();
    });

    afterEach(() => {
      restore();
    });

    function grip(title: string): HTMLElement {
      return screen.getByRole('button', { name: `Move ${title}` });
    }

    function announced(text: string) {
      return waitFor(() => expect(screen.getByText(text)).toBeInTheDocument());
    }

    it('moves a card to the next column by keyboard and says so', async () => {
      const handlers = renderBoard();
      const handle = grip('Write the spec');
      handle.focus();

      await pressKey(handle, 'Space');
      await announced('Picked up Write the spec, in Planned.');
      await pressKey(handle, 'ArrowRight');
      await announced('Write the spec is over In progress.');
      await pressKey(handle, 'Space');
      await announced('Write the spec moved to In progress.');

      expect(handlers.move).toHaveBeenCalledTimes(1);
      expect(handlers.move).toHaveBeenCalledWith(TASKS[0], 'in_progress');
    });

    it('marks the board as dragging only while a card is carried', async () => {
      renderBoard();
      const handle = grip('Write the spec');
      handle.focus();
      expect(board()).not.toHaveAttribute('data-dragging');

      await pressKey(handle, 'Space');
      await announced('Picked up Write the spec, in Planned.');
      expect(board()).toHaveAttribute('data-dragging');

      await pressKey(handle, 'Escape');
      await announced('Moving Write the spec cancelled. It stays in Planned.');
      expect(board()).not.toHaveAttribute('data-dragging');
    });

    describe('relative times', () => {
      const NOW = new Date('2026-11-10T12:00:00Z');
      const MINUTE_MS = 60_000;
      const AGED = TASKS.map((item) => ({ ...item, updated_at: '2026-11-10T11:55:00Z' }));
      let timers: ReturnType<typeof spyOn>;

      function ticks(): (() => void)[] {
        return timers.mock.calls
          .filter((call: unknown[]) => call[1] === MINUTE_MS)
          .map((call: unknown[]) => call[0] as () => void);
      }

      function ages(): (string | null)[] {
        return Array.from(board().querySelectorAll('time'), (time) => time.textContent);
      }

      beforeEach(() => {
        setSystemTime(NOW);
        timers = spyOn(globalThis, 'setInterval');
      });

      afterEach(() => {
        timers.mockRestore();
        setSystemTime();
      });

      it('refreshes every card from one timer for the whole board', async () => {
        render(
          <Board
            tasks={AGED}
            projects={PROJECTS}
            renaming={null}
            removed={null}
            view={new View()}
            actions={actions()}
          />
        );
        expect(ages()).toEqual(AGED.map(() => '5m5 minutes ago'));
        expect(ticks()).toHaveLength(1);

        setSystemTime(new Date(NOW.getTime() + 2 * MINUTE_MS));
        await act(async () => {
          ticks()[0]();
        });

        expect(ages()).toEqual(AGED.map(() => '7m7 minutes ago'));
        expect(ticks()).toHaveLength(1);
      });

      it('stops ticking while a card is carried and starts again once it is put down', async () => {
        const stopped = spyOn(globalThis, 'clearInterval');
        render(
          <Board
            tasks={AGED}
            projects={PROJECTS}
            renaming={null}
            removed={null}
            view={new View()}
            actions={actions()}
          />
        );
        const handle = grip('Write the spec');
        handle.focus();
        const resting =
          timers.mock.results[
            timers.mock.calls.indexOf(
              timers.mock.calls.find((call: unknown[]) => call[1] === MINUTE_MS)
            )
          ].value;

        await pressKey(handle, 'Space');
        await announced('Picked up Write the spec, in Planned.');
        expect(stopped).toHaveBeenCalledWith(resting);
        expect(ticks()).toHaveLength(1);

        await pressKey(handle, 'Escape');
        await announced('Moving Write the spec cancelled. It stays in Planned.');
        expect(ticks()).toHaveLength(2);
        stopped.mockRestore();
      });
    });

    it('moves a card back to Planned as created', async () => {
      const handlers = renderBoard();
      const handle = grip('Ship it');
      handle.focus();

      await pressKey(handle, 'Space');
      await announced('Picked up Ship it, in In review.');
      for (let step = 0; step < 4; step += 1) await pressKey(handle, 'ArrowLeft');
      await announced('Ship it is over Planned.');
      await pressKey(handle, 'Enter');
      await announced('Ship it moved to Planned.');

      expect(handlers.move).toHaveBeenCalledWith(TASKS[3], 'created');
    });

    it('leaves a card dropped on its own column alone', async () => {
      const handlers = renderBoard();
      const handle = grip('Queue the build');
      handle.focus();

      await pressKey(handle, 'Space');
      await announced('Picked up Queue the build, in Planned.');
      await pressKey(handle, 'Space');
      await announced('Queue the build stays in Planned.');

      expect(handlers.move).not.toHaveBeenCalled();
    });

    it('stays put past the last column', async () => {
      const handlers = renderBoard();
      const handle = grip('Stop the old table');
      handle.focus();

      await pressKey(handle, 'Space');
      await announced('Picked up Stop the old table, in Cancelled.');
      await pressKey(handle, 'ArrowRight');
      await pressKey(handle, 'Space');
      await announced('Stop the old table stays in Cancelled.');

      expect(handlers.move).not.toHaveBeenCalled();
    });

    async function carryRight(title: string) {
      const handle = grip(title);
      handle.focus();
      await pressKey(handle, 'Space');
      await announced(`Picked up ${title}, in Planned.`);
      await pressKey(handle, 'ArrowRight');
      await announced(`${title} is over In progress.`);
      await pressKey(handle, 'Space');
      await announced(`${title} moved to In progress.`);
    }

    it('keeps focus on the grip of a card that moved to another column', async () => {
      render(<MovingBoard answer={Promise.resolve(true)} />);

      await carryRight('Write the spec');

      expect(within(column('In progress')).getByText('Write the spec')).toBeInTheDocument();
      expect(document.activeElement).toBe(grip('Write the spec'));
      expect(screen.getByText('Write the spec moved to In progress.')).toBeInTheDocument();
    });

    it('keeps focus on the grip of a card whose move was refused, and says it is back', async () => {
      let refuse: () => void = () => {};
      const answer = new Promise<boolean>((resolve) => {
        refuse = () => resolve(false);
      });
      render(<MovingBoard answer={answer} />);

      await carryRight('Write the spec');
      expect(within(column('In progress')).getByText('Write the spec')).toBeInTheDocument();
      expect(document.activeElement).toBe(grip('Write the spec'));

      await act(async () => {
        refuse();
      });

      expect(within(column('Planned')).getByText('Write the spec')).toBeInTheDocument();
      expect(document.activeElement).toBe(grip('Write the spec'));
      await announced('Write the spec could not be moved; back in Planned.');
    });

    it('puts a card back when the move is cancelled', async () => {
      const handlers = renderBoard();
      const handle = grip('Build the board');
      handle.focus();

      await pressKey(handle, 'Space');
      await announced('Picked up Build the board, in In progress.');
      await pressKey(handle, 'ArrowRight');
      await announced('Build the board is over Blocked.');
      await pressKey(handle, 'Escape');
      await announced('Moving Build the board cancelled. It stays in In progress.');

      expect(handlers.move).not.toHaveBeenCalled();
    });
  });
});
