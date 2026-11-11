import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { openSelect } from '../../../test/select';
import type { Task, UpdateTaskRequest } from '../types';
import { TaskDetail } from './TaskDetail';

const task: Task = {
  id: 'task-1',
  workspace_id: 'workspace-1',
  project_ids: [],
  title: 'Ship the board',
  description: 'Replace the table with a board',
  acceptance_criteria: null,
  status: 'created',
  priority: 3,
  model_name: null,
  dependencies: [],
  created_at: '2026-11-01T00:00:00Z',
  updated_at: '2026-11-01T00:00:00Z',
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

function renderDetail(onSave: (request: UpdateTaskRequest) => Promise<void>) {
  return render(
    <TaskDetail
      task={task}
      tasks={[task]}
      projects={[]}
      sources={[]}
      onOpen={() => {}}
      onClose={() => {}}
      onExecute={() => {}}
      onSave={onSave}
    />
  );
}

function other(id: string, title: string, overrides: Partial<Task> = {}): Task {
  return { ...task, id, title, ...overrides };
}

function renderLinked(edited: Task, others: Task[] = []) {
  const onOpen = mock((_task: Task) => {});
  const onSave = mock((_request: UpdateTaskRequest) => Promise.resolve());
  render(
    <TaskDetail
      task={edited}
      tasks={[edited, ...others]}
      projects={[]}
      sources={[]}
      onOpen={onOpen}
      onClose={() => {}}
      onExecute={() => {}}
      onSave={onSave}
    />
  );
  return { onOpen, onSave };
}

function links(): HTMLElement {
  return screen.getByRole('region', { name: 'Links' });
}

function description(control: HTMLElement): string | null | undefined {
  return document.getElementById(control.getAttribute('aria-describedby') ?? '')?.textContent;
}

describe('TaskDetail', () => {
  it('lists every status in order with the shared labels', async () => {
    renderDetail(() => Promise.resolve());

    openSelect('Status');

    const options = await screen.findAllByRole('option');
    expect(options.map((option) => option.textContent)).toEqual([
      'Created',
      'Queued',
      'In progress',
      'Blocked',
      'Paused',
      'In review',
      'Completed',
      'Cancelled',
    ]);
  });

  it('saves only the status when paused is chosen', async () => {
    const onSave = mock((_request: UpdateTaskRequest) => Promise.resolve());
    renderDetail(onSave);

    openSelect('Status');
    fireEvent.click(await screen.findByRole('option', { name: 'Paused' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0]?.[0]).toEqual({ status: 'paused' });
  });

  it('says the save failed when the failure carries no message', async () => {
    renderDetail(() => Promise.reject('offline'));

    openSelect('Status');
    fireEvent.click(await screen.findByRole('option', { name: 'Paused' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(await screen.findByText('Failed to save task')).toBeInTheDocument();
  });

  describe('links', () => {
    const LONG =
      'Migrate the notification service from the legacy queue to the event bus without dropping messages';

    it('shows no Links section for a task with nothing linked', () => {
      renderLinked(task, [other('task-2', 'Write the spec')]);

      expect(screen.queryByRole('region', { name: 'Links' })).not.toBeInTheDocument();
      expect(screen.queryByRole('heading', { name: 'Links' })).not.toBeInTheDocument();
      expect(screen.queryByText('Depends on')).not.toBeInTheDocument();
      expect(screen.queryByText('Blocks')).not.toBeInTheDocument();
    });

    it('shows no Links section for a pull request address that cannot be linked', () => {
      renderLinked({ ...task, pr_url: 'javascript:alert(1)' });

      expect(screen.queryByRole('region', { name: 'Links' })).not.toBeInTheDocument();
    });

    it('links the pull request and names its branch', () => {
      renderLinked({
        ...task,
        pr_url: 'https://github.com/zone/app/pull/220',
        pr_status: 'open',
        branch_name: 'feature/task-links',
      });

      expect(within(links()).getByRole('heading', { name: 'Links', level: 3 })).toBeInTheDocument();
      const link = within(links()).getByRole('link', { name: 'PR #220 · open' });
      expect(link).toHaveAttribute('href', 'https://github.com/zone/app/pull/220');
      expect(link).toHaveAttribute('target', '_blank');
      expect(link).toHaveAttribute('rel', 'noopener noreferrer');
      expect(within(link).getByText('PR #220 · open')).toHaveClass('ui-badge-success');
      expect(within(links()).getByText('feature/task-links')).toBeInTheDocument();
      expect(within(links()).queryByText('Depends on')).not.toBeInTheDocument();
    });

    it('shows a pull request status that has no address without a link', () => {
      renderLinked({ ...task, pr_status: 'pending' });

      expect(within(links()).getByText('PR · pending')).toHaveClass('ui-badge-neutral');
      expect(within(links()).queryByRole('link')).not.toBeInTheDocument();
    });

    it('shows the status without a link for a pull request address that is not on the web', () => {
      renderLinked({ ...task, pr_status: 'open', pr_url: 'javascript:alert(1)' });

      expect(within(links()).getByText('PR · open')).toBeInTheDocument();
      expect(links().querySelector('a')).toBeNull();
    });

    it('names the branch of a task that has no pull request yet', () => {
      renderLinked({ ...task, branch_name: 'feature/auth' });

      expect(within(links()).getByText('feature/auth')).toBeInTheDocument();
      expect(within(links()).queryByText(/^PR/)).not.toBeInTheDocument();
    });

    it('lists every task it depends on and every task it blocks, in full, with status', () => {
      const dependencies = [
        other('spec', LONG, { status: 'complete' }),
        other('api', 'Design the API', { status: 'in_progress' }),
        other('auth', 'Add sign in', { status: 'blocked' }),
        other('data', 'Model the data', { status: 'paused' }),
      ];
      const dependents = [
        other('ship', 'Ship it', { status: 'review', dependencies: ['task-1'] }),
        other('tell', 'Announce it', { status: 'cancelled', dependencies: ['task-1', 'ship'] }),
      ];
      renderLinked({ ...task, dependencies: dependencies.map((item) => item.id) }, [
        ...dependencies,
        ...dependents,
      ]);

      const depends = within(within(links()).getByRole('list', { name: 'Depends on' }));
      const opens = depends.getAllByRole('button');
      expect(opens.map((button) => button.getAttribute('aria-label'))).toEqual([
        `Open ${LONG}`,
        'Open Design the API',
        'Open Add sign in',
        'Open Model the data',
      ]);
      expect(opens[0].textContent).toBe(LONG);
      expect(opens.map(description)).toEqual(['Completed', 'In progress', 'Blocked', 'Paused']);
      expect(opens[2].querySelector('svg')).toHaveClass('status-icon-blocked');
      expect(depends.queryByText(/more$/)).not.toBeInTheDocument();

      const blocks = within(within(links()).getByRole('list', { name: 'Blocks' }));
      expect(blocks.getAllByRole('button').map((button) => button.textContent)).toEqual([
        'Ship it',
        'Announce it',
      ]);
      expect(blocks.getAllByRole('button').map(description)).toEqual(['In review', 'Cancelled']);
    });

    it('counts a dependency that is not loaded instead of naming it', () => {
      renderLinked({ ...task, dependencies: ['spec', 'gone', 'deleted'] }, [
        other('spec', 'Write the spec'),
      ]);

      const depends = within(within(links()).getByRole('list', { name: 'Depends on' }));
      expect(depends.getAllByRole('button')).toHaveLength(1);
      expect(depends.getByText('2 unknown')).toBeInTheDocument();
    });

    it('opens the related task that was clicked', () => {
      const spec = other('spec', 'Write the spec');
      const ship = other('ship', 'Ship it', { dependencies: ['task-1'] });
      const { onOpen, onSave } = renderLinked({ ...task, dependencies: ['spec'] }, [spec, ship]);

      fireEvent.click(within(links()).getByRole('button', { name: 'Open Ship it' }));
      expect(onOpen).toHaveBeenCalledTimes(1);
      expect(onOpen).toHaveBeenCalledWith(ship);

      fireEvent.click(within(links()).getByRole('button', { name: 'Open Write the spec' }));
      expect(onOpen).toHaveBeenCalledTimes(2);
      expect(onOpen).toHaveBeenLastCalledWith(spec);
      expect(onSave).not.toHaveBeenCalled();
    });

    it('asks before a related task replaces unsaved edits', async () => {
      const ship = other('ship', 'Ship it', { dependencies: ['task-1'] });
      const { onOpen } = renderLinked(task, [ship]);
      const field = screen.getByLabelText('Description');
      fireEvent.change(field, { target: { value: 'Draft nobody saved' } });

      fireEvent.click(within(links()).getByRole('button', { name: 'Open Ship it' }));
      const dialog = await screen.findByRole('dialog', {
        name: 'Discard your changes to Ship the board?',
      });
      expect(onOpen).not.toHaveBeenCalled();

      fireEvent.click(within(dialog).getByRole('button', { name: 'Keep editing' }));
      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(onOpen).not.toHaveBeenCalled();
      expect(screen.getByLabelText('Description')).toHaveValue('Draft nobody saved');

      fireEvent.click(within(links()).getByRole('button', { name: 'Open Ship it' }));
      fireEvent.click(
        within(await screen.findByRole('dialog')).getByRole('button', { name: 'Discard changes' })
      );
      expect(onOpen).toHaveBeenCalledTimes(1);
      expect(onOpen).toHaveBeenCalledWith(ship);
    });

    it('does not count a stored description that only differs by surrounding space as an edit', () => {
      const ship = other('ship', 'Ship it', { dependencies: ['task-1'] });
      const { onOpen } = renderLinked({ ...task, description: `${task.description}\n` }, [ship]);

      expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
      fireEvent.click(within(links()).getByRole('button', { name: 'Open Ship it' }));

      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
      expect(onOpen).toHaveBeenCalledWith(ship);
    });

    it('drops the question when the editor moves to another task', async () => {
      const ship = other('ship', 'Ship it', { dependencies: ['task-1'] });
      const props = {
        tasks: [task, ship],
        projects: [],
        sources: [],
        onOpen: mock((_task: Task) => {}),
        onClose: () => {},
        onExecute: () => {},
        onSave: () => Promise.resolve(),
      };
      const { rerender } = render(<TaskDetail task={task} {...props} />);
      fireEvent.change(screen.getByLabelText('Description'), { target: { value: 'Unsaved' } });
      fireEvent.click(within(links()).getByRole('button', { name: 'Open Ship it' }));
      await screen.findByRole('dialog');

      rerender(<TaskDetail task={ship} {...props} />);

      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(props.onOpen).not.toHaveBeenCalled();
    });

    it('offers nothing to edit in the Links section', () => {
      renderLinked({ ...task, dependencies: ['spec'], branch_name: 'feature/auth' }, [
        other('spec', 'Write the spec'),
      ]);

      expect(links().querySelector('input, select, textarea')).toBeNull();
      expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
    });

    it('reaches the links before the fields with the keyboard', () => {
      renderLinked({ ...task, dependencies: ['spec'] }, [other('spec', 'Write the spec')]);

      const open = screen.getByRole('button', { name: 'Open Write the spec' });
      const field = screen.getByLabelText('Description');
      expect(open.compareDocumentPosition(field) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    });
  });
});
