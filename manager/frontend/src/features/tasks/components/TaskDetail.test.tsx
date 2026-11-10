import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
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
      projects={[]}
      sources={[]}
      onClose={() => {}}
      onExecute={() => {}}
      onSave={onSave}
    />
  );
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
});
