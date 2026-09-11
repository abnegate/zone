import { beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { Question } from '../../chats/types';
import type { TaskRun } from '../types';

const mockAnswerRun = mock((_runId: string, _answers: unknown[]) => Promise.resolve({} as TaskRun));

mock.module('../../../api/tasks', () => ({
  tasksApi: { answerRun: mockAnswerRun },
}));

const { QuestionPrompt } = await import('./QuestionPrompt');

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
    { label: 'Other', description: 'Something else.', recommended: false, free_text: true },
  ],
  multi_select: false,
  required: true,
};

const waiting: TaskRun = {
  id: 'run-1',
  task_id: 'task-1',
  status: 'waiting',
  current_phase: 'acting',
  progress_percent: 40,
  error_message: null,
  pending_question: { tool_call_id: 'call-1', questions: [question] },
};

describe('QuestionPrompt', () => {
  beforeEach(() => {
    mockAnswerRun.mockReset();
    mockAnswerRun.mockImplementation(() => Promise.resolve(waiting));
  });

  it('renders the question the run parked on', () => {
    render(<QuestionPrompt run={waiting} onAnswered={() => {}} />);

    expect(screen.getByRole('heading', { name: 'The run needs an answer' })).toBeInTheDocument();
    expect(screen.getByText('Scope')).toBeInTheDocument();
    expect(screen.getByText('How far should this go?')).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'Backfill' })).toBeEnabled();
  });

  it('says what happens to an unanswered run, which the reader cannot see from the card', () => {
    render(<QuestionPrompt run={waiting} onAnswered={() => {}} />);

    expect(screen.getByText(/optional question/)).toHaveTextContent(/thirty seconds/);
    const required = screen.getByText(/required question/);
    expect(required).toHaveTextContent(/an hour/);
    expect(required).toHaveTextContent(/not retried/);
    expect(required).toHaveTextContent(/start a new run/);
  });

  it('renders nothing when a waiting run carries no readable question', () => {
    const { container } = render(
      <QuestionPrompt run={{ ...waiting, pending_question: null }} onAnswered={() => {}} />
    );

    expect(container.firstChild).toBeNull();
  });

  it('posts the structured answers and refreshes the run', async () => {
    const onAnswered = mock(() => {});
    render(<QuestionPrompt run={waiting} onAnswered={onAnswered} />);

    fireEvent.click(screen.getByRole('radio', { name: 'Forward only' }));
    fireEvent.click(screen.getByTestId('question-submit'));

    await waitFor(() => expect(onAnswered).toHaveBeenCalledTimes(1));
    expect(mockAnswerRun).toHaveBeenCalledWith('run-1', [
      { header: 'Scope', labels: ['Forward only'] },
    ]);
  });

  it('sends the typed text as the free-text answer rather than a rendered line', async () => {
    render(<QuestionPrompt run={waiting} onAnswered={() => {}} />);

    fireEvent.click(screen.getByRole('radio', { name: 'Other' }));
    fireEvent.change(screen.getByTestId('question-free-text'), {
      target: { value: 'after the release' },
    });
    fireEvent.click(screen.getByTestId('question-submit'));

    await waitFor(() =>
      expect(mockAnswerRun).toHaveBeenCalledWith('run-1', [
        { header: 'Scope', labels: ['Other'], other: 'after the release' },
      ])
    );
  });

  it('disables the card once the run is no longer waiting on anyone', () => {
    render(<QuestionPrompt run={{ ...waiting, status: 'running' }} onAnswered={() => {}} />);

    expect(screen.getByRole('radio', { name: 'Backfill' })).toBeDisabled();
    expect(screen.getByTestId('question-submit')).toBeDisabled();
    expect(screen.getByTestId('question-submit')).toHaveTextContent('Answered');
  });

  it('shows a rejected answer inline and leaves the choices as the reader made them', async () => {
    mockAnswerRun.mockImplementation(() => Promise.reject(new Error('Answer a required question')));
    const onAnswered = mock(() => {});
    render(<QuestionPrompt run={waiting} onAnswered={onAnswered} />);

    fireEvent.click(screen.getByRole('radio', { name: 'Backfill' }));
    fireEvent.click(screen.getByTestId('question-submit'));

    expect(await screen.findByRole('alert')).toHaveTextContent('Answer a required question');
    expect(onAnswered).not.toHaveBeenCalled();
    expect(screen.getByRole('radio', { name: 'Backfill' })).toBeChecked();
    expect(screen.getByTestId('question-submit')).toBeEnabled();
  });

  it('sends one answer while a send is still in flight', async () => {
    let settle!: (run: TaskRun) => void;
    mockAnswerRun.mockImplementation(
      () =>
        new Promise<TaskRun>((done) => {
          settle = done;
        })
    );
    render(<QuestionPrompt run={waiting} onAnswered={() => {}} />);

    fireEvent.click(screen.getByRole('radio', { name: 'Backfill' }));
    const submit = screen.getByTestId('question-submit');
    fireEvent.click(submit);

    await waitFor(() => expect(submit).toBeDisabled());
    fireEvent.click(submit);
    expect(mockAnswerRun).toHaveBeenCalledTimes(1);
    settle(waiting);
    await waitFor(() => expect(submit).toBeEnabled());
  });
});
