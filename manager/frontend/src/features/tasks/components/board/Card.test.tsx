import { afterEach, beforeEach, describe, expect, it, mock, setSystemTime } from 'bun:test';
import { DndContext, useSensor, useSensors } from '@dnd-kit/core';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ReactNode } from 'react';
import { chooseMenuItem, openMenu, settleMenu } from '../../../../test/menu';
import { type Relations, UNRELATED } from '../../relations';
import { TitleDraft } from '../../TitleDraft';
import type { BoardActions, Task, TaskStatus } from '../../types';
import { Card } from './Card';
import { CardSensor } from './CardSensor';

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
    move: mock(() => Promise.resolve(true)),
  };
}

function Board({ children, onDragStart }: { children: ReactNode; onDragStart?: () => void }) {
  const sensors = useSensors(useSensor(CardSensor, { activationConstraint: { distance: 5 } }));
  return (
    <DndContext sensors={sensors} onDragStart={onDragStart}>
      <ul>{children}</ul>
    </DndContext>
  );
}

interface CardOptions {
  renaming?: boolean;
  projects?: string;
  relations?: Relations;
  onDragStart?: () => void;
}

function other(id: string, title: string, status: TaskStatus = 'created'): Task {
  return { ...base, id, title, status };
}

function renderCard(
  overrides: Partial<Task> = {},
  { renaming = false, projects = PROJECTS, relations = UNRELATED, onDragStart }: CardOptions = {}
) {
  const task = { ...base, ...overrides };
  const actions = makeActions();
  const view = render(
    <Board onDragStart={onDragStart}>
      <Card
        task={task}
        projects={projects}
        relations={relations}
        now={NOW.getTime()}
        renaming={renaming ? new TitleDraft(task.title) : null}
        actions={actions}
      />
    </Board>
  );
  return { task, actions, view };
}

function card(): HTMLElement {
  const item = document.querySelector<HTMLElement>('li.task-card');
  if (!item) throw new Error('No card is rendered');
  return item;
}

function relation(label: string): HTMLElement {
  return screen.getByRole('list', { name: label });
}

function description(control: HTMLElement): string | null | undefined {
  return document.getElementById(control.getAttribute('aria-describedby') ?? '')?.textContent;
}

function drag(target: HTMLElement, pointerType = 'mouse') {
  fireEvent.pointerDown(target, {
    isPrimary: true,
    button: 0,
    clientX: 10,
    clientY: 10,
    pointerType,
  });
  fireEvent.pointerMove(target, { isPrimary: true, clientX: 40, clientY: 10, pointerType });
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

  it('leaves the projects out when the task has none', () => {
    renderCard({ project_ids: [] }, { projects: '' });
    expect(card().querySelector('.task-card-project')).toBeNull();
    expect(screen.queryByText('No projects')).not.toBeInTheDocument();
  });

  it('leaves the details row empty when the task has nothing to put in it', () => {
    renderCard({ project_ids: [], updated_at: null, created_at: null }, { projects: '' });
    expect(card().querySelector('.task-card-meta')?.childNodes).toHaveLength(0);
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
    it(`tints a ${status} pull request chip that has no address yet, and does not link it`, () => {
      renderCard({ pr_status: status });
      expect(screen.getByText(`PR · ${status}`)).toHaveClass(tint);
      expect(screen.queryByRole('link')).not.toBeInTheDocument();
    });

    it(`tints a linked ${status} pull request chip`, () => {
      renderCard({ pr_status: status, pr_url: 'https://github.com/zone/app/pull/7' });
      const link = screen.getByRole('link', { name: `PR #7 · ${status}` });
      expect(within(link).getByText(`PR #7 · ${status}`)).toHaveClass(tint);
    });
  }

  it('links the one pull request chip in a new tab without an opener', () => {
    const { actions } = renderCard({
      pr_status: 'open',
      pr_url: 'https://github.com/zone/app/pull/220',
    });
    const link = screen.getByRole('link', { name: 'PR #220 · open' });
    expect(link).toHaveAttribute('href', 'https://github.com/zone/app/pull/220');
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');
    expect(screen.getAllByText(/PR/)).toHaveLength(1);
    expect(screen.queryByText('View PR')).not.toBeInTheDocument();
    fireEvent.click(link);
    expect(actions.open).not.toHaveBeenCalled();
  });

  it('leaves the number out of a pull request address that has none', () => {
    renderCard({ pr_status: 'merged', pr_url: 'https://review.example.com/changes/latest' });
    expect(screen.getByRole('link', { name: 'PR · merged' })).toHaveAttribute(
      'href',
      'https://review.example.com/changes/latest'
    );
  });

  for (const address of ['javascript:alert(document.cookie)', 'data:text/html,hello', 'pull/7']) {
    it(`shows the status without a link for a pull request address of ${address}`, () => {
      renderCard({ pr_status: 'open', pr_url: address });
      expect(screen.getByText('PR · open')).toHaveClass('ui-badge-success');
      expect(screen.queryByRole('link')).not.toBeInTheDocument();
      expect(card().querySelector('a')).toBeNull();
    });

    it(`shows no pull request for an address of ${address} that has no status`, () => {
      renderCard({ pr_url: address });
      expect(screen.queryByText(/PR/)).not.toBeInTheDocument();
      expect(card().querySelector('a')).toBeNull();
    });
  }

  it('links to a pull request served over plain http, untinted while it has no status', () => {
    renderCard({ pr_url: 'http://git.internal/zone/app/merge_requests/7' });
    const link = screen.getByRole('link', { name: 'PR #7' });
    expect(link).toHaveAttribute('href', 'http://git.internal/zone/app/merge_requests/7');
    expect(within(link).getByText('PR #7')).toHaveClass('ui-badge-neutral');
  });

  it('shows no pull request on a task without one', () => {
    renderCard();
    expect(screen.queryByText(/PR/)).not.toBeInTheDocument();
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });

  it('shows no relations on a task without any', () => {
    renderCard();
    expect(screen.queryByText('Depends on')).not.toBeInTheDocument();
    expect(screen.queryByText('Blocks')).not.toBeInTheDocument();
    expect(card().querySelector('ul')).toBeNull();
    expect(screen.queryByRole('button', { name: /^Open / })).not.toBeInTheDocument();
  });

  it('lists the tasks it depends on and the tasks it blocks, each with its status', () => {
    renderCard(
      {},
      {
        relations: {
          dependencies: [
            other('spec', 'Write the spec', 'complete'),
            other('api', 'Design the API', 'in_progress'),
          ],
          dependents: [other('ship', 'Ship it', 'queued')],
          unknown: 0,
        },
      }
    );

    const dependencies = within(relation('Depends on')).getAllByRole('button');
    expect(dependencies.map((button) => button.getAttribute('aria-label'))).toEqual([
      'Open Write the spec',
      'Open Design the API',
    ]);
    expect(dependencies.map((button) => button.textContent)).toEqual([
      'Write the spec',
      'Design the API',
    ]);
    expect(dependencies.map(description)).toEqual(['Completed', 'In progress']);
    expect(dependencies[0].querySelector('svg')).toHaveClass('status-icon-complete');
    expect(dependencies[1].querySelector('svg')).toHaveClass('status-icon-in_progress');
    expect(dependencies[0]).toHaveAttribute('title', 'Write the spec');

    const dependents = within(relation('Blocks')).getAllByRole('button');
    expect(dependents.map((button) => button.getAttribute('aria-label'))).toEqual(['Open Ship it']);
    expect(dependents.map(description)).toEqual(['Queued']);
    expect(dependents[0].querySelector('svg')).toHaveClass('status-icon-planned');
    expect(within(relation('Blocks')).getAllByRole('listitem')).toHaveLength(1);
  });

  it('shows one relation without the other', () => {
    renderCard(
      {},
      { relations: { ...UNRELATED, dependents: [other('ship', 'Ship it', 'review')] } }
    );
    expect(relation('Blocks')).toBeInTheDocument();
    expect(screen.queryByText('Depends on')).not.toBeInTheDocument();
  });

  it('names three related tasks and counts the rest', () => {
    const many = ['One', 'Two', 'Three', 'Four', 'Five'].map((title) =>
      other(title.toLowerCase(), title)
    );
    renderCard({}, { relations: { dependencies: many, dependents: many.slice(0, 4), unknown: 0 } });

    const dependencies = within(relation('Depends on'));
    expect(dependencies.getAllByRole('button').map((button) => button.textContent)).toEqual([
      'One',
      'Two',
      'Three',
    ]);
    expect(dependencies.getByText('+2 more')).toBeInTheDocument();
    const dependents = within(relation('Blocks'));
    expect(dependents.getAllByRole('button')).toHaveLength(3);
    expect(dependents.getByText('+1 more')).toBeInTheDocument();
  });

  it('names exactly three related tasks without counting a rest', () => {
    const three = ['One', 'Two', 'Three'].map((title) => other(title.toLowerCase(), title));
    renderCard({}, { relations: { ...UNRELATED, dependencies: three } });
    expect(within(relation('Depends on')).getAllByRole('listitem')).toHaveLength(3);
    expect(screen.queryByText(/more$/)).not.toBeInTheDocument();
  });

  it('counts a dependency that is not loaded instead of naming it', () => {
    renderCard(
      { dependencies: ['spec', 'gone', 'deleted'] },
      { relations: { ...UNRELATED, dependencies: [other('spec', 'Write the spec')], unknown: 2 } }
    );
    const dependencies = within(relation('Depends on'));
    expect(dependencies.getAllByRole('button')).toHaveLength(1);
    expect(dependencies.getByText('2 unknown')).toBeInTheDocument();
    expect(screen.queryByText(/gone|deleted/)).not.toBeInTheDocument();
  });

  it('counts the dependencies of a task whose every dependency is unknown', () => {
    renderCard({ dependencies: ['gone'] }, { relations: { ...UNRELATED, unknown: 1 } });
    expect(within(relation('Depends on')).getByText('1 unknown')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Open / })).not.toBeInTheDocument();
  });

  it('counts the hidden and the unknown dependencies apart', () => {
    const many = ['One', 'Two', 'Three', 'Four'].map((title) => other(title.toLowerCase(), title));
    renderCard({}, { relations: { ...UNRELATED, dependencies: many, unknown: 2 } });
    expect(within(relation('Depends on')).getByText('+1 more, 2 unknown')).toBeInTheDocument();
  });

  it('opens the related task, not the card it is listed on', () => {
    const spec = other('spec', 'Write the spec', 'complete');
    const { actions } = renderCard({}, { relations: { ...UNRELATED, dependencies: [spec] } });
    fireEvent.click(screen.getByRole('button', { name: 'Open Write the spec' }));
    expect(actions.open).toHaveBeenCalledTimes(1);
    expect(actions.open).toHaveBeenCalledWith(spec);
  });

  it('keeps the related tasks between the title and the menu for the keyboard', () => {
    renderCard(
      { pr_url: 'https://github.com/zone/app/pull/7' },
      {
        relations: {
          dependencies: [other('spec', 'Write the spec')],
          dependents: [other('ship', 'Ship it')],
          unknown: 0,
        },
      }
    );
    const controls = Array.from(card().querySelectorAll('button, a'));
    expect(
      controls.map((control) => control.getAttribute('aria-label') ?? control.textContent)
    ).toEqual([
      'Move Implement login',
      'Implement login',
      'PR #7',
      'Open Write the spec',
      'Open Ship it',
      'Actions for Implement login',
    ]);
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

  it('never dates an update ahead of the clock the board last read', () => {
    renderCard({ updated_at: '2026-11-10T12:00:20Z' });
    const time = card().querySelector('time');
    expect(time).toHaveAttribute('dateTime', '2026-11-10T12:00:20Z');
    expect(time).toHaveTextContent('less than a minute ago');
  });

  it('keeps tooltips off the details the open button covers', () => {
    renderCard({ priority: 2 });
    expect(card().querySelector('time')).not.toHaveAttribute('title');
    expect(card().querySelector('.task-card-project')).not.toHaveAttribute('title');
    expect(screen.getByLabelText('Priority 2')).not.toHaveAttribute('title');
    expect(screen.getByRole('button', { name: 'Implement login' })).toHaveAttribute(
      'title',
      'Implement login'
    );
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

  it('starts a pen drag from the card body', async () => {
    const onDragStart = mock();
    renderCard({}, { onDragStart });
    drag(screen.getByRole('button', { name: 'Implement login' }), 'pen');
    await waitFor(() => expect(onDragStart).toHaveBeenCalled());
  });

  it('leaves a finger on the card body to the browser, so a swipe scrolls', async () => {
    const onDragStart = mock();
    renderCard({}, { onDragStart });
    drag(screen.getByRole('button', { name: 'Implement login' }), 'touch');
    await settleMenu();
    expect(onDragStart).not.toHaveBeenCalled();
    expect(card()).not.toHaveClass('task-card-dragging');
  });

  it('starts a touch drag from the grip handle', async () => {
    const onDragStart = mock();
    renderCard({}, { onDragStart });
    drag(screen.getByRole('button', { name: 'Move Implement login' }), 'touch');
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
    drag(screen.getByRole('link', { name: 'PR #7' }));
    await settleMenu();
    expect(onDragStart).not.toHaveBeenCalled();
  });

  it('does not start a drag from a related task', async () => {
    const onDragStart = mock();
    renderCard(
      {},
      {
        relations: { ...UNRELATED, dependencies: [other('spec', 'Write the spec')] },
        onDragStart,
      }
    );
    drag(screen.getByRole('button', { name: 'Open Write the spec' }));
    await settleMenu();
    expect(onDragStart).not.toHaveBeenCalled();
    expect(card()).not.toHaveClass('task-card-dragging');
  });

  it('still starts a pointer drag from the body of a card that lists related tasks', async () => {
    const onDragStart = mock();
    renderCard(
      {},
      {
        relations: { ...UNRELATED, dependencies: [other('spec', 'Write the spec')] },
        onDragStart,
      }
    );
    drag(screen.getByText('Depends on'));
    await waitFor(() => expect(onDragStart).toHaveBeenCalled());
  });

  it('does not start a drag while the title is being renamed', async () => {
    const onDragStart = mock();
    renderCard({}, { renaming: true, onDragStart });
    drag(screen.getByRole('textbox', { name: 'Task title' }));
    await settleMenu();
    expect(onDragStart).not.toHaveBeenCalled();
  });
});
