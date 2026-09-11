import { beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { Question } from '../../chats/types';
import type { Task, TaskRun, TaskRunLog } from '../types';

const question: Question = {
  header: 'Scope',
  question: 'How far should this go?',
  choices: [
    {
      label: 'Backfill',
      description: 'Rewrite every existing row.',
      recommended: true,
      free_text: false,
    },
    {
      label: 'Forward only',
      description: 'Leave the existing rows alone.',
      recommended: false,
      free_text: false,
    },
  ],
  multi_select: false,
  required: true,
};

const running: TaskRun = {
  id: 'run-1',
  task_id: 'task-1',
  status: 'running',
  current_phase: 'acting',
  progress_percent: 40,
  error_message: null,
  started_at: '2026-09-10T00:00:00Z',
  completed_at: null,
};

const waiting: TaskRun = {
  ...running,
  status: 'waiting',
  pending_question: { tool_call_id: 'call-1', questions: [question] },
};

const mockRunTask = mock(() => Promise.resolve(running));
const mockGetTaskRuns = mock(() => Promise.resolve([] as TaskRun[]));
const mockGetTaskRun = mock(() => Promise.resolve(running));
const mockGetTaskRunLogs = mock(() => Promise.resolve([] as TaskRunLog[]));
const mockAnswerRun = mock((_runId: string, _answers: unknown[]) => Promise.resolve(running));

mock.module('../../../api/tasks', () => ({
  tasksApi: {
    runTask: mockRunTask,
    getTaskRuns: mockGetTaskRuns,
    getTaskRun: mockGetTaskRun,
    getTaskRunLogs: mockGetTaskRunLogs,
    answerRun: mockAnswerRun,
  },
}));

const { TaskExecutionView } = await import('./TaskExecutionView');

const task: Task = {
  id: 'task-1',
  workspace_id: 'workspace-1',
  project_ids: [],
  title: 'Migrate the rows',
  description: 'Move every row to the new shape',
  acceptance_criteria: null,
  status: 'in_progress',
  priority: 1,
  model_name: null,
  dependencies: [],
  created_at: '2026-09-10T00:00:00Z',
  updated_at: '2026-09-10T00:00:00Z',
  started_at: null,
  completed_at: null,
  is_agentic: true,
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

describe('a task run that parked on a question', () => {
  beforeEach(() => {
    mockRunTask.mockReset();
    mockGetTaskRuns.mockReset();
    mockGetTaskRun.mockReset();
    mockGetTaskRunLogs.mockReset();
    mockAnswerRun.mockReset();
    mockGetTaskRuns.mockImplementation(() => Promise.resolve([waiting]));
    mockGetTaskRun.mockImplementation(() => Promise.resolve(waiting));
    mockGetTaskRunLogs.mockImplementation(() => Promise.resolve([]));
    mockAnswerRun.mockImplementation(() => Promise.resolve(running));
  });

  it('says the run is waiting for the reader rather than still running', async () => {
    render(<TaskExecutionView task={task} onClose={() => {}} />);

    expect(await screen.findByText('Waiting for you')).toBeInTheDocument();
    expect(screen.queryByText('Running')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Run Again' })).not.toBeInTheDocument();
  });

  it('asks the question above the log the run stopped in', async () => {
    render(<TaskExecutionView task={task} onClose={() => {}} />);

    expect(await screen.findByTestId('question-card')).toBeInTheDocument();
    expect(screen.getByText('How far should this go?')).toBeInTheDocument();
    const card = screen.getByTestId('question-card');
    const logs = screen.getByRole('region', { name: 'Execution logs' });
    expect(card.compareDocumentPosition(logs) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('keeps polling a waiting run, so an answer from elsewhere still lands here', async () => {
    render(<TaskExecutionView task={task} onClose={() => {}} />);

    expect(await screen.findByTestId('question-card')).toBeInTheDocument();
    mockGetTaskRun.mockImplementation(() =>
      Promise.resolve({ ...running, status: 'completed', current_phase: null })
    );
    await waitFor(() => expect(screen.getByText('Completed')).toBeInTheDocument(), {
      timeout: 4000,
    });
    expect(mockGetTaskRun.mock.calls.length).toBeGreaterThan(1);
    expect(screen.queryByTestId('question-card')).not.toBeInTheDocument();
  });

  it('shows no card for a waiting run whose question could not be read', async () => {
    mockGetTaskRuns.mockImplementation(() =>
      Promise.resolve([{ ...waiting, pending_question: undefined }])
    );
    mockGetTaskRun.mockImplementation(() =>
      Promise.resolve({ ...waiting, pending_question: undefined })
    );
    render(<TaskExecutionView task={task} onClose={() => {}} />);

    expect(await screen.findByText('Waiting for you')).toBeInTheDocument();
    expect(screen.queryByTestId('question-card')).not.toBeInTheDocument();
  });

  it('answers the parked run and reloads it rather than trusting the local card', async () => {
    render(<TaskExecutionView task={task} onClose={() => {}} />);

    fireEvent.click(await screen.findByRole('radio', { name: 'Backfill' }));
    mockGetTaskRuns.mockImplementation(() => Promise.resolve([running]));
    mockGetTaskRun.mockImplementation(() => Promise.resolve(running));
    fireEvent.click(screen.getByTestId('question-submit'));

    await waitFor(() =>
      expect(mockAnswerRun).toHaveBeenCalledWith('run-1', [
        { header: 'Scope', labels: ['Backfill'] },
      ])
    );
    expect(await screen.findByText('Running')).toBeInTheDocument();
    expect(screen.queryByTestId('question-card')).not.toBeInTheDocument();
  });
});
