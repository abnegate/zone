import { useEffect, useState } from 'react';
import type {
  JobExited,
  JobStarted,
  ToolCallRecord,
  Waiting,
  WaitSettled,
  WaitVerdict,
} from '../types';
import './JobCard.css';

const RUNNING_LABEL = 'Job running';
const EXITED_LABEL = 'Job exited with code';
const KILLED_LABEL = 'Job killed without exiting';
const WAITING_LABEL = 'Waiting for';
const SETTLED_LABEL = 'Finished waiting';
const TIMED_OUT_LABEL = 'Timed out — not a result';
const UNKNOWN_LABEL = 'Nothing reported — not a pass';
const UNREADABLE_LABEL = 'Checks could not be read — not a pass';
const DEADLINE_PASSED_LABEL = 'Deadline passed';

const KIND_JOB = 'job';
const KIND_TASK_RUN = 'task_run';
const KIND_CHECK = 'check';

const SUCCESSFUL_EXIT_CODE = 0;

type JobState = 'running' | 'ok' | 'failed';
type WaitState = 'waiting' | 'settled' | 'timed-out' | 'unknown' | 'unreadable';

const WAIT_STATES: Record<WaitVerdict, WaitState> = {
  settled: 'settled',
  timed_out: 'timed-out',
  silent: 'unknown',
  unreadable: 'unreadable',
};

/// A job that has not reported an exit is running as far as this client knows;
/// one that exited is a pass only on a clean code. A killed job has no code,
/// which the frozen shape uses to mean exactly that, and it is never a pass.
function jobState(exited?: JobExited): JobState {
  if (!exited) return 'running';
  return exited.exit_code === SUCCESSFUL_EXIT_CODE ? 'ok' : 'failed';
}

function jobTitle(exited?: JobExited): string {
  if (!exited) return RUNNING_LABEL;
  if (exited.exit_code === undefined) return KILLED_LABEL;
  return `${EXITED_LABEL} ${exited.exit_code}`;
}

/// The outcome is prose written for the model, so the card reads nothing out of
/// it: the server says which way the wait ended, and this draws that. Deciding
/// from the words is how an outcome opening "The checks on…" — the one that
/// exists to say the checks are not known to have passed — was drawn with the
/// same neutral title as a check that came back green.
function waitState(settled?: WaitSettled): WaitState {
  return settled ? WAIT_STATES[settled.verdict] : 'waiting';
}

function waitSubject(waiting: Waiting): string {
  switch (waiting.kind) {
    case KIND_JOB:
      return waiting.id;
    case KIND_TASK_RUN:
      return `task run ${waiting.id}`;
    case KIND_CHECK:
      return `checks on ${waiting.reference ?? waiting.id}`;
    default:
      return `${waiting.kind} ${waiting.id}`;
  }
}

function waitTitle(state: WaitState, waiting?: Waiting): string {
  switch (state) {
    case 'timed-out':
      return TIMED_OUT_LABEL;
    case 'unknown':
      return UNKNOWN_LABEL;
    case 'unreadable':
      return UNREADABLE_LABEL;
    case 'settled':
      return SETTLED_LABEL;
    case 'waiting':
      return waiting ? `${WAITING_LABEL} ${waitSubject(waiting)}` : WAITING_LABEL;
  }
}

function formatDeadline(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' });
}

function formatRemaining(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(seconds % 60).padStart(2, '0')} left`;
}

/// Seconds until the deadline, ticking once a second until it passes. Null
/// when the deadline cannot be read, in which case the raw value is shown and
/// nothing counts down to it.
function useRemaining(deadline: string): number | null {
  const target = new Date(deadline).getTime();
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (Number.isNaN(target) || target <= Date.now()) return;
    const interval = window.setInterval(() => {
      const current = Date.now();
      setNow(current);
      if (current >= target) window.clearInterval(interval);
    }, 1000);
    return () => window.clearInterval(interval);
  }, [target]);

  if (Number.isNaN(target)) return null;
  return Math.max(0, Math.ceil((target - now) / 1000));
}

function Deadline({ deadline }: { deadline: string }) {
  const remaining = useRemaining(deadline);

  return (
    <>
      <time dateTime={deadline}>{formatDeadline(deadline)}</time>
      {remaining !== null ? (
        <span
          className="job-card-countdown"
          role="timer"
          aria-label="Time remaining"
          data-testid="wait-countdown"
        >
          {remaining > 0 ? formatRemaining(remaining) : DEADLINE_PASSED_LABEL}
        </span>
      ) : null}
    </>
  );
}

function Job({ job, exited }: { job?: JobStarted; exited?: JobExited }) {
  const state = jobState(exited);

  return (
    <div className={`job-card job-card--${state}`} data-testid="job-card">
      <p className="job-card-header">
        <span className="job-card-status" aria-hidden="true" />
        <span className="job-card-title">{jobTitle(exited)}</span>
      </p>
      <dl className="job-card-meta">
        <div>
          <dt>Job</dt>
          <dd className="job-card-mono">{job?.id ?? exited?.id}</dd>
        </div>
        {job ? (
          <div>
            <dt>Process</dt>
            <dd className="job-card-mono">{job.pid}</dd>
          </div>
        ) : null}
        {job ? (
          <div className="job-card-wide">
            <dt>Log</dt>
            <dd className="job-card-mono">{job.log_path}</dd>
          </div>
        ) : null}
      </dl>
    </div>
  );
}

function Wait({ waiting, settled }: { waiting?: Waiting; settled?: WaitSettled }) {
  const state = waitState(settled);

  return (
    <div className={`job-card job-card--${state}`} data-testid="wait-card">
      <p className="job-card-header">
        <span className="job-card-status" aria-hidden="true" />
        <span className="job-card-title">{waitTitle(state, waiting)}</span>
      </p>
      {waiting ? (
        <dl className="job-card-meta">
          <div>
            <dt>Id</dt>
            <dd className="job-card-mono">{waiting.id}</dd>
          </div>
          {waiting.reference ? (
            <div>
              <dt>Reference</dt>
              <dd className="job-card-mono">{waiting.reference}</dd>
            </div>
          ) : null}
          {settled ? null : (
            <div className="job-card-wide">
              <dt>Until</dt>
              <dd>
                <Deadline deadline={waiting.deadline} />
              </dd>
            </div>
          )}
        </dl>
      ) : null}
      {settled ? (
        <p className="job-card-outcome" data-testid="wait-outcome">
          {settled.outcome}
        </p>
      ) : null}
    </div>
  );
}

/// The background work behind a tool call: the job it detached, the wait it
/// opened, and how either ended. Keyed on what the record carries, never on
/// the tool's name, so an exit or a settle that reached the trace without its
/// start still has a card.
export function JobCard({
  call,
}: {
  call: Pick<ToolCallRecord, 'job' | 'exited' | 'waiting' | 'settled'>;
}) {
  const { job, exited, waiting, settled } = call;

  return (
    <>
      {job || exited ? <Job job={job} exited={exited} /> : null}
      {waiting || settled ? <Wait waiting={waiting} settled={settled} /> : null}
    </>
  );
}
