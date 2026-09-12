import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import {
  type JobExited,
  type JobStarted,
  UNKNOWN_CHECKS_OUTCOME_PREFIX,
  type Waiting,
  type WaitSettled,
} from '../types';
import { JobCard } from './JobCard';

const FAR_FUTURE = '2099-01-01T00:00:00Z';
const LONG_PAST = '2000-01-01T00:00:00Z';

const started: JobStarted = {
  id: 'job_9f3c1a7b2e04',
  pid: 48213,
  log_path: '/srv/zone/.zone/jobs/job_9f3c1a7b2e04.log',
};

const exited = (overrides: Partial<JobExited> = {}): JobExited => ({
  id: 'job_9f3c1a7b2e04',
  ...overrides,
});

const waiting = (overrides: Partial<Waiting> = {}): Waiting => ({
  kind: 'job',
  id: 'job_9f3c1a7b2e04',
  deadline: FAR_FUTURE,
  ...overrides,
});

const settled = (overrides: Partial<WaitSettled> = {}): WaitSettled => ({
  tool_call_id: 'call_wait',
  outcome: 'job_9f3c1a7b2e04 exited with code 0 after 214s.',
  timed_out: false,
  ...overrides,
});

/// Anything a reader could take for a pass: the one green state, or a mark.
function expectNotAPass(card: HTMLElement) {
  expect(card).not.toHaveClass('job-card--ok');
  expect(card.textContent).not.toContain('✓');
}

describe('JobCard', () => {
  it('renders nothing for a call that started nothing and waited for nothing', () => {
    const { container } = render(<JobCard call={{}} />);

    expect(container.firstChild).toBeNull();
  });

  it('shows a running job with the id, process and log a reader would look up', () => {
    render(<JobCard call={{ job: started }} />);

    const card = screen.getByTestId('job-card');
    expect(card).toHaveClass('job-card--running');
    expect(screen.getByText('Job running')).toBeInTheDocument();
    expect(screen.getByText('job_9f3c1a7b2e04')).toBeInTheDocument();
    expect(screen.getByText('48213')).toBeInTheDocument();
    expect(screen.getByText('/srv/zone/.zone/jobs/job_9f3c1a7b2e04.log')).toBeInTheDocument();
    expect(screen.queryByTestId('wait-card')).not.toBeInTheDocument();
  });

  it('shows a clean exit as the one state that is a pass', () => {
    render(<JobCard call={{ job: started, exited: exited({ exit_code: 0 }) }} />);

    expect(screen.getByTestId('job-card')).toHaveClass('job-card--ok');
    expect(screen.getByText('Job exited with code 0')).toBeInTheDocument();
  });

  it('shows a failing exit code as a failure', () => {
    render(<JobCard call={{ job: started, exited: exited({ exit_code: 1 }) }} />);

    const card = screen.getByTestId('job-card');
    expect(card).toHaveClass('job-card--failed');
    expectNotAPass(card);
    expect(screen.getByText('Job exited with code 1')).toBeInTheDocument();
  });

  it('shows a job killed without an exit code as a failure, never a pass', () => {
    render(<JobCard call={{ job: started, exited: exited() }} />);

    const card = screen.getByTestId('job-card');
    expect(card).toHaveClass('job-card--failed');
    expectNotAPass(card);
    expect(screen.getByText('Job killed without exiting')).toBeInTheDocument();
  });

  it('still shows an exit that arrived without the start it belongs to', () => {
    render(<JobCard call={{ exited: exited({ exit_code: 0 }) }} />);

    expect(screen.getByTestId('job-card')).toHaveClass('job-card--ok');
    expect(screen.getByText('job_9f3c1a7b2e04')).toBeInTheDocument();
    expect(screen.queryByText('Process')).not.toBeInTheDocument();
    expect(screen.queryByText('Log')).not.toBeInTheDocument();
  });

  it('shows a wait in progress with its subject and the deadline it counts down to', () => {
    render(<JobCard call={{ waiting: waiting() }} />);

    const card = screen.getByTestId('wait-card');
    expect(card).toHaveClass('job-card--waiting');
    expect(screen.getByText('Waiting for job_9f3c1a7b2e04')).toBeInTheDocument();
    expect(card.querySelector('time')).toHaveAttribute('datetime', FAR_FUTURE);
    expect(screen.getByTestId('wait-countdown')).toHaveTextContent(/\d+:\d{2} left/);
    expect(screen.queryByTestId('job-card')).not.toBeInTheDocument();
  });

  it('says the deadline has passed rather than counting up from nothing', () => {
    render(<JobCard call={{ waiting: waiting({ deadline: LONG_PAST }) }} />);

    expect(screen.getByTestId('wait-countdown')).toHaveTextContent('Deadline passed');
    expect(screen.getByTestId('wait-card')).toHaveClass('job-card--waiting');
  });

  it('shows an unreadable deadline as written and counts nothing down', () => {
    render(<JobCard call={{ waiting: waiting({ deadline: 'soon' }) }} />);

    expect(screen.getByText('soon')).toBeInTheDocument();
    expect(screen.queryByTestId('wait-countdown')).not.toBeInTheDocument();
  });

  it('names what each kind of wait is for', () => {
    const { rerender } = render(
      <JobCard
        call={{
          waiting: waiting({ kind: 'task_run', id: '2f1c9e8a-0b44-4d7e-9c31-5a6b7c8d9e0f' }),
        }}
      />
    );
    expect(
      screen.getByText('Waiting for task run 2f1c9e8a-0b44-4d7e-9c31-5a6b7c8d9e0f')
    ).toBeInTheDocument();

    rerender(
      <JobCard call={{ waiting: waiting({ kind: 'check', id: '8c4d21fa', reference: 'main' }) }} />
    );
    expect(screen.getByText('Waiting for checks on main')).toBeInTheDocument();
    expect(screen.getByText('main')).toBeInTheDocument();

    rerender(<JobCard call={{ waiting: waiting({ kind: 'comet', id: 'halley' }) }} />);
    expect(screen.getByText('Waiting for comet halley')).toBeInTheDocument();
  });

  it('renders a wait that timed out as a timeout, not a success', () => {
    const outcome =
      'Timed out after 300s. job_9f3c1a7b2e04 has not finished — this is a timeout, not a result. Check again or wait longer.';
    render(
      <JobCard call={{ waiting: waiting(), settled: settled({ outcome, timed_out: true }) }} />
    );

    const card = screen.getByTestId('wait-card');
    expect(card).toHaveClass('job-card--timed-out');
    expectNotAPass(card);
    expect(screen.getByText('Timed out — not a result')).toBeInTheDocument();
    expect(screen.getByTestId('wait-outcome')).toHaveTextContent(outcome);
    expect(screen.queryByTestId('wait-countdown')).not.toBeInTheDocument();
  });

  it('renders a commit nothing reported on as not a pass', () => {
    const outcome = `${UNKNOWN_CHECKS_OUTCOME_PREFIX} main after 120s. This is not a pass.`;
    render(
      <JobCard
        call={{
          waiting: waiting({ kind: 'check', id: '8c4d21fa', reference: 'main' }),
          settled: settled({ outcome }),
        }}
      />
    );

    const card = screen.getByTestId('wait-card');
    expect(card).toHaveClass('job-card--unknown');
    expectNotAPass(card);
    expect(screen.getByText('Nothing reported — not a pass')).toBeInTheDocument();
    expect(screen.getByTestId('wait-outcome')).toHaveTextContent(outcome);
  });

  it('shows any other settle in its own words without reading a pass out of them', () => {
    render(<JobCard call={{ waiting: waiting(), settled: settled() }} />);

    const card = screen.getByTestId('wait-card');
    expect(card).toHaveClass('job-card--settled');
    expectNotAPass(card);
    expect(screen.getByText('Finished waiting')).toBeInTheDocument();
    expect(screen.getByTestId('wait-outcome')).toHaveTextContent(
      'job_9f3c1a7b2e04 exited with code 0 after 214s.'
    );
  });

  it('still shows a settle that arrived without the wait it belongs to', () => {
    render(<JobCard call={{ settled: settled({ timed_out: true }) }} />);

    expect(screen.getByTestId('wait-card')).toHaveClass('job-card--timed-out');
    expect(screen.getByTestId('wait-outcome')).toBeInTheDocument();
  });
});
