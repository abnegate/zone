import { afterEach, beforeEach, describe, expect, it, mock, setSystemTime } from 'bun:test';
import { DndContext, PointerSensor, useSensor, useSensors } from '@dnd-kit/core';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { chooseMenuItem, openMenu, settleMenu } from '../../../../test/menu';
import type { BoardActions, Task } from '../../types';
import { Card } from './Card';

const NOW = new Date('2026-11-10T12:00:00Z');
const PROJECTS = 'Website, Mobile';
const MENU = 'Actions for Implement login';

const base: Task = {
  id: 'task-1',
  workspace_id: 'workspace-1',
  project_ids: ['proj-1', 'proj-2'],
  title: 'Implement login',
  description: 'Add user authentication',
  acceptance_criteria: null,
  status: 'created',
  priority: null,
  model_name: null,
  dependencies: [],
  created_at: '2026-11-01T12:00:00Z',
  updated_at: '2026-11-10T09:00:00Z',
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

function makeActions(): BoardActions {
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

function Board({ children, onDragStart }: { children: ReactNode; onDragStart?: () => void }) {
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 5 } }));
  return (
    <DndContext sensors={sensors} onDragStart={onDragStart}>
      <ul>{children}</ul>
    </DndContext>
  );
}

function renderCard(
  overrides: Partial<Task> = {},
  { renaming = false, onDragStart }: { renaming?: boolean; onDragStart?: () => void } = {}
) {
  const task = { ...base, ...overrides };
  const actions = makeActions();
  const view = render(
    <Board onDragStart={onDragStart}>
      <Card task={task} projects={PROJECTS} renaming={renaming} actions={actions} />
    </Board>
  );
  return { task, actions, view };
}

function card(): HTMLElement {
  return screen.getByRole('listitem', { hidden: true });
}

function drag(target: HTMLElement) {
  fireEvent.pointerDown(target, { isPrimary: true, button: 0, clientX: 10, clientY: 10 });
  fireEvent.pointerMove(target, { isPrimary: true, clientX: 40, clientY: 10 });
}

describe('Card', () => {
  beforeEach(() => {
    setSystemTime(NOW);
  });

  afterEach(() => {
    setSystemTime();
  });

  it('opens the task from its title button', () => {
    const { task, actions } = renderCard();
    fireEvent.click(screen.getByRole('button', { name: 'Implement login' }));
    expect(actions.open).toHaveBeenCalledWith(task);
  });

  it('opens the task from the keyboard with Enter on the title button', () => {
    const { actions } = renderCard();
    const title = screen.getByRole('button', { name: 'Implement login' });
    title.focus();
    fireEvent.keyDown(title, { key: 'Enter', code: 'Enter' });
    fireEvent.click(title);
    expect(actions.open).toHaveBeenCalledTimes(1);
  });

  it('marks a title still being generated as busy', () => {
    renderCard({ title_pending: true, created_at: '2026-11-10T11:59:30Z' });
    const title = screen.getByRole('button', { name: 'Implement login' });
    expect(title).toHaveAttribute('aria-busy', 'true');
    expect(title).toHaveClass('task-card-open', 'task-card-title-pending');
  });

  it('leaves a settled title unmarked', () => {
    renderCard();
    const title = screen.getByRole('button', { name: 'Implement login' });
    expect(title).not.toHaveAttribute('aria-busy');
    expect(title).not.toHaveClass('task-card-title-pending');
  });

  it('shows the Queued chip only on queued tasks', () => {
    const { view } = renderCard({ status: 'queued' });
    expect(screen.getByText('Queued')).toHaveClass('ui-badge-info');
    view.unmount();
    renderCard({ status: 'created' });
    expect(screen.queryByText('Queued')).not.toBeInTheDocument();
  });

  it('shows the project names', () => {
    renderCard();
    expect(card().querySelector('.task-card-project')).toHaveTextContent(PROJECTS);
  });

  it('shows a priority chip named for screen readers', () => {
    renderCard({ priority: 2 });
    expect(screen.getByLabelText('Priority 2')).toHaveTextContent('P2');
  });

  it('leaves the priority chip out when the task has none', () => {
    renderCard({ priority: null });
    expect(screen.queryByLabelText(/^Priority/)).not.toBeInTheDocument();
  });

  it('badges agentic tasks and marks the card', () => {
    renderCard({ is_agentic: true });
    expect(screen.getByText('Agentic')).toHaveClass('ui-badge-accent');
    expect(card()).toHaveClass('task-card', 'task-card-agentic');
  });

  it('leaves manual tasks without the agentic badge', () => {
    renderCard();
    expect(screen.queryByText('Agentic')).not.toBeInTheDocument();
    expect(card()).not.toHaveClass('task-card-agentic');
  });

  for (const [status, tint] of [
    ['pending', 'ui-badge-neutral'],
    ['open', 'ui-badge-success'],
    ['merged', 'ui-badge-accent'],
    ['closed', 'ui-badge-destructive'],
  ] as const) {
    it(`tints a ${status} pull request badge`, () => {
      renderCard({ pr_status: status });
      expect(screen.getByText(`PR ${status}`)).toHaveClass(tint);
    });
  }

  it('links to the pull request in a new tab without an opener', () => {
    const { actions } = renderCard({
      pr_status: 'open',
      pr_url: 'https://github.com/zone/app/pull/7',
    });
    const link = screen.getByRole('link', { name: 'View PR' });
    expect(link).toHaveAttribute('href', 'https://github.com/zone/app/pull/7');
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');
    fireEvent.click(link);
    expect(actions.open).not.toHaveBeenCalled();
  });

  it('shows when the task was last updated relative to now', () => {
    renderCard();
    const time = card().querySelector('time');
    expect(time).toHaveAttribute('dateTime', '2026-11-10T09:00:00Z');
    expect(time).toHaveTextContent('about 3 hours ago');
  });

  it('falls back to the creation time when the task was never updated', () => {
    renderCard({ updated_at: null });
    const time = card().querySelector('time');
    expect(time).toHaveAttribute('dateTime', '2026-11-01T12:00:00Z');
    expect(time).toHaveTextContent('9 days ago');
  });

  it('leaves the time out when the task has no timestamps', () => {
    renderCard({ updated_at: null, created_at: null });
    expect(card().querySelector('time')).toBeNull();
  });

  it('leaves the time out when the timestamp does not parse', () => {
    renderCard({ updated_at: 'not a date' });
    expect(card().querySelector('time')).toBeNull();
  });

  for (const [item, action] of [
    ['Run', 'run'],
    ['Rename', 'rename'],
    ['Delete', 'remove'],
  ] as const) {
    it(`calls ${action} from the menu's ${item} item without opening the task`, async () => {
      const { task, actions } = renderCard();
      await chooseMenuItem(MENU, item);
      expect(actions[action]).toHaveBeenCalledWith(task);
      expect(actions.open).not.toHaveBeenCalled();
    });
  }

  it('lists Run, Rename and a destructive Delete in the menu', async () => {
    renderCard();
    await openMenu(MENU);
    const items = screen.getAllByRole('menuitem');
    expect(items.map((item) => item.textContent)).toEqual(['Run', 'Rename', 'Delete']);
    expect(items[2]).toHaveClass('ui-action-menu-item-destructive');
  });

  it('swaps the title for an input while renaming and saves the new title', async () => {
    const { task, actions } = renderCard({}, { renaming: true });
    expect(screen.queryByRole('button', { name: 'Implement login' })).not.toBeInTheDocument();
    const input = screen.getByRole('textbox', { name: 'Task title' });
    expect(input).toHaveValue('Implement login');
    fireEvent.change(input, { target: { value: 'Implement sign in' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(actions.finish).toHaveBeenCalled());
    expect(actions.retitle).toHaveBeenCalledWith(task, 'Implement sign in');
    expect(actions.open).not.toHaveBeenCalled();
  });

  it('labels the grip handle for moving the task and keeps it from opening the task', () => {
    const { actions } = renderCard();
    const handle = screen.getByRole('button', { name: 'Move Implement login' });
    expect(handle).toHaveClass('task-card-handle');
    expect(handle).toHaveAttribute('aria-roledescription', 'draggable');
    fireEvent.click(handle);
    expect(actions.open).not.toHaveBeenCalled();
  });

  it('starts a pointer drag from the card body', async () => {
    const onDragStart = mock();
    renderCard({}, { onDragStart });
    drag(screen.getByRole('button', { name: 'Implement login' }));
    await waitFor(() => expect(onDragStart).toHaveBeenCalled());
    await waitFor(() => expect(card()).toHaveClass('task-card-dragging'));
  });

  it('swallows the click that ends a pointer drag so the task does not open', async () => {
    const onDragStart = mock();
    const { actions } = renderCard({}, { onDragStart });
    const title = screen.getByRole('button', { name: 'Implement login' });
    drag(title);
    await waitFor(() => expect(onDragStart).toHaveBeenCalled());
    fireEvent.pointerUp(title, { isPrimary: true, clientX: 40, clientY: 10 });
    fireEvent.click(title);
    expect(actions.open).not.toHaveBeenCalled();
  });

  it('does not start a drag from the menu trigger, open or closed', async () => {
    const onDragStart = mock();
    renderCard({}, { onDragStart });
    const trigger = await openMenu(MENU);
    drag(trigger);
    await settleMenu();
    drag(trigger);
    await settleMenu();
    expect(onDragStart).not.toHaveBeenCalled();
    expect(card()).not.toHaveClass('task-card-dragging');
  });

  it('does not start a drag from the pull request link', async () => {
    const onDragStart = mock();
    renderCard({ pr_url: 'https://github.com/zone/app/pull/7' }, { onDragStart });
    drag(screen.getByRole('link', { name: 'View PR' }));
    await settleMenu();
    expect(onDragStart).not.toHaveBeenCalled();
  });

  it('does not start a drag while the title is being renamed', async () => {
    const onDragStart = mock();
    renderCard({}, { renaming: true, onDragStart });
    drag(screen.getByRole('textbox', { name: 'Task title' }));
    await settleMenu();
    expect(onDragStart).not.toHaveBeenCalled();
  });
});
