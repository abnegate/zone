import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import type { ComponentProps } from 'react';
import { BrowserRouter } from 'react-router-dom';
import type { AutomationTask, ProjectAutomation } from '../types';
import { AutomationPanel } from './AutomationPanel';

const task = (overrides: Partial<AutomationTask>): AutomationTask => ({
  task_id: 'task-1',
  title: 'A task',
  status: 'created',
  is_agentic: true,
  kind: 'feature',
  stage: null,
  reason: null,
  runs: 0,
  review_rounds: 0,
  reviewers: null,
  pr_url: null,
  head: null,
  checks: null,
  merge_sha: null,
  auto_created: false,
  ...overrides,
});

const automation = (overrides: Partial<ProjectAutomation>): ProjectAutomation => ({
  project_id: 'proj-1',
  auto: true,
  actor_id: 'user-1',
  paused_reason: null,
  completed_at: null,
  parallelism: 3,
  planner_chat_id: null,
  updates_chat_id: 'chat-u',
  counts: { total: 3, agentic: 3, complete: 1, in_flight: 1, paused: 0 },
  tasks: [],
  ...overrides,
});

function renderPanel(props: Partial<ComponentProps<typeof AutomationPanel>>) {
  return render(
    <BrowserRouter>
      <AutomationPanel
        automation={null}
        loading={false}
        error={null}
        resuming={false}
        onResume={() => {}}
        {...props}
      />
    </BrowserRouter>
  );
}

describe('AutomationPanel', () => {
  it('shows a spinner while the first read is in flight', () => {
    renderPanel({ loading: true });
    expect(screen.getByTestId('automation-panel')).toHaveTextContent('Reading automation');
  });

  it('shows the error when nothing was read', () => {
    renderPanel({ error: 'Not found' });
    expect(screen.getByTestId('automation-panel')).toHaveTextContent('Not found');
  });

  it('lists every task with its stage, reviewers and pull request', () => {
    renderPanel({
      automation: automation({
        planner_chat_id: 'chat-p',
        tasks: [
          task({
            task_id: 't1',
            title: 'Scaffold',
            kind: 'scaffold',
            status: 'complete',
            stage: 'merged',
            runs: 1,
            review_rounds: 2,
            reviewers: 'reviewer-a, CodeRabbit',
            pr_url: 'https://github.com/acme/app/pull/1',
            reason: 'stale reason that merged tasks hide',
          }),
          task({
            task_id: 't2',
            title: 'Login',
            stage: 'fixing',
            runs: 2,
            reason: 'r1-1: missing test for the empty password case',
          }),
          task({ task_id: 't3', title: 'Write the launch post', is_agentic: false }),
        ],
      }),
    });

    expect(screen.getByText('1 of 3 tasks merged, 1 in flight')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Interview' })).toHaveAttribute(
      'href',
      '/chats?id=chat-p'
    );
    expect(screen.getByRole('link', { name: 'Updates' })).toHaveAttribute(
      'href',
      '/chats?id=chat-u'
    );
    expect(screen.getByText('Merged')).toBeInTheDocument();
    expect(screen.getByText('Fixing review findings')).toBeInTheDocument();
    expect(screen.getByText('Manual')).toBeInTheDocument();
    expect(screen.getByText('2 review rounds')).toBeInTheDocument();
    expect(screen.getByText('Reviewed by reviewer-a, CodeRabbit')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Pull request' })).toHaveAttribute(
      'href',
      'https://github.com/acme/app/pull/1'
    );
    expect(screen.getByText('r1-1: missing test for the empty password case')).toBeInTheDocument();
    expect(screen.queryByText('stale reason that merged tasks hide')).not.toBeInTheDocument();
    expect(screen.queryByTestId('automation-paused')).not.toBeInTheDocument();
  });

  it('offers Resume with the reason when the project paused', () => {
    const onResume = mock(() => {});
    renderPanel({
      onResume,
      automation: automation({ paused_reason: 'The actor lost write access' }),
    });

    expect(screen.getByTestId('automation-paused')).toHaveTextContent(
      'The actor lost write access'
    );
    fireEvent.click(screen.getByRole('button', { name: 'Resume' }));
    expect(onResume).toHaveBeenCalled();
  });

  it('offers Resume when only tasks paused', () => {
    renderPanel({
      automation: automation({
        counts: { total: 3, agentic: 3, complete: 1, in_flight: 0, paused: 2 },
      }),
    });
    expect(screen.getByTestId('automation-paused')).toHaveTextContent('2 tasks need a person.');
  });

  it('disables Resume while resuming', () => {
    renderPanel({
      resuming: true,
      automation: automation({ paused_reason: 'Paused' }),
    });
    expect(screen.getByRole('button', { name: 'Resuming…' })).toBeDisabled();
  });

  it('says when automation finished', () => {
    renderPanel({
      automation: automation({
        completed_at: '2026-01-01T00:00:00Z',
        counts: { total: 3, agentic: 3, complete: 3, in_flight: 0, paused: 0 },
      }),
    });
    expect(screen.getByText('Every agentic task merged; automation finished.')).toBeInTheDocument();
  });
});
