import { beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { Question, Waiting } from '../../chats/types';
import type { Task, TaskRun, TaskRunLog } from '../types';

const FAR_FUTURE = '2099-01-01T00:00:00Z';
const LONG_PAST = '2000-01-01T00:00:00Z';
const WAITING_ON_OUTCOME = 'Task run is waiting on something outside its loop';

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

const check: Waiting = {
  kind: 'check',
  id: 'a1b2c3d4e5f6',
  reference: 'main',
  deadline: FAR_FUTURE,
};

const parked: TaskRun = {
  ...running,
  status: 'waiting',
  current_phase: 'waiting',
  waiting_on: check,
};

const parkLine: TaskRunLog = {
  id: 'log-2',
  phase: 'waiting',
  agent_type: 'agent',
  level: 'info',
  message: WAITING_ON_OUTCOME,
  metadata: { tool_call_id: 'call-2', waiting: check },
  created_at: '2026-09-10T00:00:02Z',
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

  it('names the phase the run is in as waiting for an answer, not the one it parked from', async () => {
    mockGetTaskRun.mockImplementation(() =>
      Promise.resolve({ ...waiting, current_phase: 'waiting' })
    );
    mockGetTaskRunLogs.mockImplementation(() =>
      Promise.resolve([
        {
          id: 'log-1',
          phase: 'waiting',
          agent_type: 'agent',
          level: 'info',
          message: 'Task run is waiting on a question',
          metadata: null,
          created_at: '2026-09-10T00:00:01Z',
        },
      ])
    );
    render(<TaskExecutionView task={task} onClose={() => {}} />);

    expect(await screen.findByText('Waiting for you')).toBeInTheDocument();
    expect(await screen.findByText('Waiting for an answer')).toBeInTheDocument();
    expect(screen.getByText('Waiting')).toBeInTheDocument();
    expect(screen.getByText('Task run is waiting on a question')).toBeInTheDocument();
    expect(screen.queryByText('Reviewing results')).not.toBeInTheDocument();
    expect(screen.queryByTestId('wait-countdown')).not.toBeInTheDocument();
  });

  it('reads as a question, not a wait, when the run carries both', async () => {
    mockGetTaskRun.mockImplementation(() =>
      Promise.resolve({ ...waiting, current_phase: 'waiting', waiting_on: check })
    );
    render(<TaskExecutionView task={task} onClose={() => {}} />);

    expect(await screen.findByTestId('question-card')).toBeInTheDocument();
    expect(screen.getByText('Waiting for you')).toBeInTheDocument();
    expect(screen.getByText('Waiting for an answer')).toBeInTheDocument();
    expect(screen.queryByText('Waiting for checks on main')).not.toBeInTheDocument();
    expect(screen.queryByTestId('wait-countdown')).not.toBeInTheDocument();
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

describe('a task run that parked on a wait', () => {
  beforeEach(() => {
    mockRunTask.mockReset();
    mockGetTaskRuns.mockReset();
    mockGetTaskRun.mockReset();
    mockGetTaskRunLogs.mockReset();
    mockAnswerRun.mockReset();
    mockGetTaskRuns.mockImplementation(() => Promise.resolve([parked]));
    mockGetTaskRun.mockImplementation(() => Promise.resolve(parked));
    mockGetTaskRunLogs.mockImplementation(() => Promise.resolve([]));
  });

  it('reads as a wait on its subject, not as a question nobody asked', async () => {
    render(<TaskExecutionView task={task} onClose={() => {}} />);

    expect(await screen.findByText('Waiting for checks on main')).toBeInTheDocument();
    expect(screen.getByText('Waiting')).toBeInTheDocument();
    expect(screen.queryByText('Waiting for you')).not.toBeInTheDocument();
    expect(screen.queryByText('Waiting for an answer')).not.toBeInTheDocument();
    expect(screen.queryByTestId('question-card')).not.toBeInTheDocument();
    expect(document.querySelector('time')).toHaveAttribute('datetime', FAR_FUTURE);
    expect(screen.getByTestId('wait-countdown')).toHaveTextContent(/\d+:\d{2} left/);
    expect(screen.queryByRole('button', { name: 'Run Again' })).not.toBeInTheDocument();
  });

  const subjects: [Waiting, string][] = [
    [{ kind: 'job', id: 'job_9f3c1a7b2e04', deadline: FAR_FUTURE }, 'job_9f3c1a7b2e04'],
    [{ kind: 'task_run', id: 'run-2', deadline: FAR_FUTURE }, 'task run run-2'],
    [{ kind: 'check', id: 'a1b2c3d4e5f6', deadline: FAR_FUTURE }, 'checks on a1b2c3d4e5f6'],
  ];
  for (const [waiting_on, subject] of subjects) {
    it(`names a ${waiting_on.kind} wait the way the chat card does`, async () => {
      mockGetTaskRuns.mockImplementation(() => Promise.resolve([{ ...parked, waiting_on }]));
      mockGetTaskRun.mockImplementation(() => Promise.resolve({ ...parked, waiting_on }));
      render(<TaskExecutionView task={task} onClose={() => {}} />);

      expect(await screen.findByText(`Waiting for ${subject}`)).toBeInTheDocument();
    });
  }

  it('says the deadline has passed rather than counting up from nothing', async () => {
    const overdue = { ...parked, waiting_on: { ...check, deadline: LONG_PAST } };
    mockGetTaskRuns.mockImplementation(() => Promise.resolve([overdue]));
    mockGetTaskRun.mockImplementation(() => Promise.resolve(overdue));
    render(<TaskExecutionView task={task} onClose={() => {}} />);

    expect(await screen.findByText('Deadline passed')).toBeInTheDocument();
    expect(screen.getByText('Waiting')).toBeInTheDocument();
  });

  it('shows an unreadable deadline as written and counts nothing down', async () => {
    const unreadable = { ...parked, waiting_on: { ...check, deadline: 'soon' } };
    mockGetTaskRuns.mockImplementation(() => Promise.resolve([unreadable]));
    mockGetTaskRun.mockImplementation(() => Promise.resolve(unreadable));
    render(<TaskExecutionView task={task} onClose={() => {}} />);

    expect(await screen.findByText('soon')).toBeInTheDocument();
    expect(screen.queryByTestId('wait-countdown')).not.toBeInTheDocument();
  });

  it('labels the park line as waiting and lets its message say on what', async () => {
    mockGetTaskRunLogs.mockImplementation(() => Promise.resolve([parkLine]));
    render(<TaskExecutionView task={task} onClose={() => {}} />);

    expect(await screen.findByText(WAITING_ON_OUTCOME)).toBeInTheDocument();
    expect(screen.getAllByText('Waiting')).toHaveLength(2);
    expect(screen.queryByText('Waiting for an answer')).not.toBeInTheDocument();
  });

  it('keeps polling a run parked on a wait, so its outcome lands here', async () => {
    render(<TaskExecutionView task={task} onClose={() => {}} />);

    expect(await screen.findByTestId('wait-countdown')).toBeInTheDocument();
    mockGetTaskRun.mockImplementation(() =>
      Promise.resolve({ ...running, status: 'completed', current_phase: null })
    );
    await waitFor(() => expect(screen.getByText('Completed')).toBeInTheDocument(), {
      timeout: 4000,
    });
    expect(mockGetTaskRun.mock.calls.length).toBeGreaterThan(1);
    expect(screen.queryByText('Waiting for checks on main')).not.toBeInTheDocument();
    expect(screen.queryByTestId('wait-countdown')).not.toBeInTheDocument();
  });
});
