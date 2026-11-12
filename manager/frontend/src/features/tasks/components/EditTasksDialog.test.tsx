import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { chooseSelect, openSelect } from '../../../test/select';
import type { Project } from '../../projects/types';
import type { Task, TaskChanges } from '../types';
import { EditTasksDialog } from './EditTasksDialog';

const task: Task = {
  id: 'task-1',
  workspace_id: 'workspace-1',
  project_ids: [],
  title: 'Ship the board',
  description: 'Replace the table',
  acceptance_criteria: null,
  status: 'created',
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

function project(id: string, name: string): Project {
  return {
    id,
    name,
    description: null,
    status: 'active',
    github_repo_url: null,
    source_id: null,
    auto: false,
    position: 0,
    created_at: '2026-11-01T00:00:00Z',
    updated_at: '2026-11-01T00:00:00Z',
  };
}

const PROJECTS = [
  project('project-1', 'Console'),
  project('project-2', 'Runner'),
  project('project-3', 'Docs'),
];

const TASKS: Task[] = [
  { ...task, id: 'task-1', project_ids: ['project-1', 'project-2'] },
  { ...task, id: 'task-2', title: 'Write the docs', project_ids: ['project-1'] },
];

const TITLE = 'Edit 2 tasks';

function deferred() {
  let resolve: () => void = () => {};
  let reject: (reason: unknown) => void = () => {};
  const promise = new Promise<void>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function renderDialog(
  onSave: (changes: TaskChanges) => Promise<void> = () => Promise.resolve(),
  tasks: readonly Task[] = TASKS
) {
  const save = mock(onSave);
  const onClose = mock(() => {});
  render(<EditTasksDialog tasks={tasks} projects={PROJECTS} onSave={save} onClose={onClose} />);
  return { onSave: save, onClose };
}

function saveButton(): HTMLButtonElement {
  return screen.getByRole('button', { name: 'Save changes' }) as HTMLButtonElement;
}

function cancelButton(): HTMLButtonElement {
  return screen.getByRole('button', { name: 'Cancel' }) as HTMLButtonElement;
}

const CHECKED = 'true';
const UNCHECKED = 'false';
const MIXED = 'mixed';

function projectBox(name: string): HTMLElement {
  return screen.getByRole('checkbox', { name });
}

function stateOf(name: string): string | null {
  return projectBox(name).getAttribute('aria-checked');
}

function optionNames(label: string): string[] {
  const trigger = openSelect(label);
  const names = screen.getAllByRole('option').map((option) => option.textContent ?? '');
  fireEvent.keyDown(trigger, { key: 'Escape' });
  return names;
}

describe('EditTasksDialog', () => {
  it('counts the tasks in the title', () => {
    renderDialog();
    expect(screen.getByRole('dialog', { name: TITLE })).toBeTruthy();
  });

  it('names a single task in the singular', () => {
    renderDialog(undefined, [TASKS[0]]);
    expect(screen.getByRole('dialog', { name: 'Edit 1 task' })).toBeTruthy();
  });

  it('starts every field unchanged with Save disabled', () => {
    renderDialog();

    for (const label of ['Status', 'Priority', 'Agentic', 'Require plan approval']) {
      expect(screen.getByRole('combobox', { name: label }).textContent).toBe('Unchanged');
    }
    expect(saveButton().disabled).toBe(true);
  });

  it('offers Unchanged before each status', () => {
    renderDialog();
    expect(optionNames('Status')).toEqual([
      'Unchanged',
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

  it('numbers the priorities beside their names', () => {
    renderDialog();
    expect(optionNames('Priority')).toEqual([
      'Unchanged',
      '1 · Lowest',
      '2 · Low',
      '3 · Medium',
      '4 · High',
      '5 · Highest',
    ]);
  });

  it('offers On and Off for the switches', () => {
    renderDialog();
    expect(optionNames('Agentic')).toEqual(['Unchanged', 'On', 'Off']);
    expect(optionNames('Require plan approval')).toEqual(['Unchanged', 'On', 'Off']);
  });

  it('saves only the status when only the status changes', async () => {
    const { onSave } = renderDialog();

    chooseSelect('Status', 'Paused');
    expect(saveButton().disabled).toBe(false);
    fireEvent.click(saveButton());

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0]).toEqual({ status: 'paused' });
  });

  it('saves priority and both switches together', async () => {
    const { onSave } = renderDialog();

    chooseSelect('Priority', '5 · Highest');
    chooseSelect('Agentic', 'Off');
    chooseSelect('Require plan approval', 'On');
    fireEvent.click(saveButton());

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0]).toEqual({
      priority: 5,
      is_agentic: false,
      require_plan_approval: true,
    });
  });

  it('drops a field chosen back to Unchanged', async () => {
    const { onSave } = renderDialog();

    chooseSelect('Status', 'Blocked');
    chooseSelect('Status', 'Unchanged');
    expect(saveButton().disabled).toBe(true);

    chooseSelect('Agentic', 'On');
    fireEvent.click(saveButton());

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0]).toEqual({ is_agentic: true });
  });

  it('marks projects by how many of the tasks share them', () => {
    renderDialog();
    const projects = screen.getByRole('group', { name: 'Projects' });

    expect(within(projects).getAllByRole('checkbox')).toHaveLength(3);
    expect(stateOf('Console')).toBe(CHECKED);
    expect(stateOf('Runner')).toBe(MIXED);
    expect(stateOf('Docs')).toBe(UNCHECKED);
  });

  it('leaves projects out until one is clicked', async () => {
    const { onSave } = renderDialog();

    chooseSelect('Priority', '2 · Low');
    fireEvent.click(saveButton());

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0]).toEqual({ priority: 2 });
  });

  it('adds a new project without touching the projects only some tasks have', async () => {
    const { onSave } = renderDialog();

    fireEvent.click(projectBox('Docs'));

    expect(stateOf('Docs')).toBe(CHECKED);
    expect(stateOf('Runner')).toBe(MIXED);
    expect(stateOf('Console')).toBe(CHECKED);
    fireEvent.click(saveButton());

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    const changes = onSave.mock.calls[0][0];
    expect(changes).toEqual({ add_project_ids: ['project-3'] });
    expect(JSON.stringify(changes)).not.toContain('project-2');
  });

  it('removes a shared project from every task with one click', async () => {
    const { onSave } = renderDialog();

    fireEvent.click(projectBox('Console'));

    expect(stateOf('Console')).toBe(UNCHECKED);
    fireEvent.click(saveButton());

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0]).toEqual({ remove_project_ids: ['project-1'] });
  });

  it('cycles a partly shared project through add, remove and back to unchanged', async () => {
    const { onSave } = renderDialog();

    fireEvent.click(projectBox('Runner'));
    expect(stateOf('Runner')).toBe(CHECKED);
    expect(saveButton().disabled).toBe(false);

    fireEvent.click(projectBox('Runner'));
    expect(stateOf('Runner')).toBe(UNCHECKED);
    expect(saveButton().disabled).toBe(false);

    fireEvent.click(projectBox('Runner'));
    expect(stateOf('Runner')).toBe(MIXED);
    expect(saveButton().disabled).toBe(true);

    fireEvent.click(projectBox('Runner'));
    fireEvent.click(saveButton());
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0]).toEqual({ add_project_ids: ['project-2'] });
  });

  it('removes a partly shared project from the tasks that have it', async () => {
    const { onSave } = renderDialog();

    fireEvent.click(projectBox('Runner'));
    fireEvent.click(projectBox('Runner'));
    fireEvent.click(saveButton());

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0]).toEqual({ remove_project_ids: ['project-2'] });
  });

  it('returns a project checked then unchecked to untouched', () => {
    renderDialog();

    fireEvent.click(projectBox('Docs'));
    expect(saveButton().disabled).toBe(false);
    fireEvent.click(projectBox('Docs'));

    expect(stateOf('Docs')).toBe(UNCHECKED);
    expect(saveButton().disabled).toBe(true);
  });

  it('returns a shared project unchecked then checked to untouched', () => {
    renderDialog();

    fireEvent.click(projectBox('Console'));
    fireEvent.click(projectBox('Console'));

    expect(stateOf('Console')).toBe(CHECKED);
    expect(saveButton().disabled).toBe(true);
  });

  it('toggles a project from its name', () => {
    renderDialog();

    fireEvent.click(screen.getByText('Docs'));

    expect(stateOf('Docs')).toBe(CHECKED);
  });

  it('sends project edits beside the other changes', async () => {
    const { onSave } = renderDialog();

    chooseSelect('Status', 'Blocked');
    fireEvent.click(projectBox('Console'));
    fireEvent.click(projectBox('Docs'));
    fireEvent.click(saveButton());

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0]).toEqual({
      status: 'blocked',
      add_project_ids: ['project-3'],
      remove_project_ids: ['project-1'],
    });
  });

  it('shows loading and locks the dialog while saving', async () => {
    const pending = deferred();
    const { onClose } = renderDialog(() => pending.promise);

    chooseSelect('Status', 'Paused');
    fireEvent.click(saveButton());

    await waitFor(() => {
      expect(saveButton().disabled).toBe(true);
      expect(saveButton().querySelector('.ui-btn-spinner')).not.toBeNull();
    });
    expect(cancelButton().disabled).toBe(true);
    expect(projectBox('Docs').hasAttribute('disabled')).toBe(true);
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole('dialog', { name: TITLE })).toBeTruthy();
    pending.resolve();
  });

  it('stays open and can retry after a failed save', async () => {
    const { onSave, onClose } = renderDialog(() =>
      Promise.reject(new Error('Task has an active run or is no longer available'))
    );

    chooseSelect('Status', 'Paused');
    fireEvent.click(saveButton());

    await waitFor(() => expect(saveButton().disabled).toBe(false));
    expect(saveButton().querySelector('.ui-btn-spinner')).toBeNull();
    expect(cancelButton().disabled).toBe(false);
    expect(screen.getByRole('dialog', { name: TITLE })).toBeTruthy();
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.click(saveButton());
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(2));
    expect(onSave.mock.calls[1][0]).toEqual({ status: 'paused' });
  });

  it('closes from Cancel without saving', () => {
    const { onSave, onClose } = renderDialog();

    fireEvent.click(cancelButton());

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onSave).not.toHaveBeenCalled();
  });

  it('closes on Escape when idle', () => {
    const { onClose } = renderDialog();

    fireEvent.keyDown(document, { key: 'Escape' });

    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
