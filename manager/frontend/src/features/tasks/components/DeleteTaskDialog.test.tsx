import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { Task } from '../types';
import { DeleteTaskDialog } from './DeleteTaskDialog';

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

function deferred() {
  let resolve: () => void = () => {};
  let reject: (reason: unknown) => void = () => {};
  const promise = new Promise<void>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function renderDialog(onConfirm: () => Promise<void>, onClose = mock(() => {})) {
  render(<DeleteTaskDialog task={task} onConfirm={onConfirm} onClose={onClose} />);
  return { onClose };
}

describe('DeleteTaskDialog', () => {
  it('names the task in the title', () => {
    renderDialog(() => Promise.resolve());
    expect(screen.getByRole('dialog', { name: 'Delete "Ship the board"?' })).toBeTruthy();
  });

  it('calls onConfirm and shows loading while deleting', async () => {
    const pending = deferred();
    const onConfirm = mock(() => pending.promise);
    renderDialog(onConfirm);

    fireEvent.click(screen.getByRole('button', { name: 'Delete task' }));

    expect(onConfirm).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      const button = screen.getByRole('button', { name: 'Delete task' }) as HTMLButtonElement;
      expect(button.disabled).toBe(true);
      expect(button.querySelector('.ui-btn-spinner')).not.toBeNull();
    });
    pending.resolve();
  });

  it('disables Cancel while deleting', async () => {
    const pending = deferred();
    renderDialog(() => pending.promise);

    fireEvent.click(screen.getByRole('button', { name: 'Delete task' }));

    await waitFor(() => {
      const cancel = screen.getByRole('button', { name: 'Cancel' }) as HTMLButtonElement;
      expect(cancel.disabled).toBe(true);
    });
    pending.resolve();
  });

  it('shows the failure and stays open', async () => {
    const onConfirm = mock(() => Promise.reject(new Error('Task has an active run')));
    const { onClose } = renderDialog(onConfirm);

    fireEvent.click(screen.getByRole('button', { name: 'Delete task' }));

    expect((await screen.findByRole('alert')).textContent).toBe('Task has an active run');
    expect(screen.getByRole('dialog', { name: 'Delete "Ship the board"?' })).toBeTruthy();
    expect(onClose).not.toHaveBeenCalled();
    const button = screen.getByRole('button', { name: 'Delete task' }) as HTMLButtonElement;
    expect(button.disabled).toBe(false);
    const cancel = screen.getByRole('button', { name: 'Cancel' }) as HTMLButtonElement;
    expect(cancel.disabled).toBe(false);
  });

  it('falls back to a generic message for non-Error failures', async () => {
    renderDialog(() => Promise.reject('nope'));

    fireEvent.click(screen.getByRole('button', { name: 'Delete task' }));

    expect((await screen.findByRole('alert')).textContent).toBe('Failed to delete the task');
  });

  it('clears the previous failure on retry', async () => {
    const pending = deferred();
    const onConfirm = mock(() => Promise.reject(new Error('Busy')));
    renderDialog(onConfirm);

    fireEvent.click(screen.getByRole('button', { name: 'Delete task' }));
    await screen.findByRole('alert');

    onConfirm.mockImplementation(() => pending.promise);
    fireEvent.click(screen.getByRole('button', { name: 'Delete task' }));

    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull());
    pending.resolve();
  });

  it('calls onClose from Cancel', () => {
    const onConfirm = mock(() => Promise.resolve());
    const { onClose } = renderDialog(onConfirm);

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
