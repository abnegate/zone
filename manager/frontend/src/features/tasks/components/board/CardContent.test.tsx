import { afterAll, beforeAll, describe, expect, it, mock } from 'bun:test';
import { render, screen } from '@testing-library/react';
import { pressKey } from '../../../../test/board';
import type { BoardActions, Task, TaskStatus } from '../../types';

const menus = new Map<string, number>();

mock.module('./CardMenu', () => ({
  CardMenu: ({ task }: { task: Task }) => {
    menus.set(task.id, (menus.get(task.id) ?? 0) + 1);
    return null;
  },
}));

let Board: typeof import('./Board').Board;
let View: typeof import('./View').View;

beforeAll(async () => {
  ({ Board } = await import('./Board'));
  ({ View } = await import('./View'));
});

afterAll(() => {
  mock.restore();
});

function task(id: string, title: string, status: TaskStatus): Task {
  return {
    id,
    workspace_id: 'workspace-1',
    project_ids: [],
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

const TASKS = [
  task('write', 'Write the spec', 'created'),
  task('build', 'Build the board', 'in_progress'),
  task('ship', 'Ship it', 'review'),
];

const ACTIONS: BoardActions = {
  open: mock(),
  run: mock(),
  rename: mock(),
  retitle: mock(() => Promise.resolve()),
  finish: mock(),
  remove: mock(),
  move: mock(() => Promise.resolve(true)),
};

describe('CardContent', () => {
  it('does not render again while a card is carried across the board', async () => {
    render(
      <Board
        tasks={TASKS}
        projects={new Map()}
        renaming={null}
        removed={null}
        view={new View()}
        actions={ACTIONS}
      />
    );
    const rendered = new Map(menus);
    expect([...rendered.keys()].sort()).toEqual(['build', 'ship', 'write']);

    const handle = screen.getByRole('button', { name: 'Move Write the spec' });
    handle.focus();
    await pressKey(handle, 'Space');
    await screen.findByText('Picked up Write the spec, in Planned.');
    await pressKey(handle, 'ArrowRight');
    await pressKey(handle, 'Escape');
    await screen.findByText('Moving Write the spec cancelled. It stays in Planned.');

    expect(menus).toEqual(rendered);
  });
});
