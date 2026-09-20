import { type ReactNode, useEffect, useState } from 'react';
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
const ENDED_LABEL = 'Job ended with the turn — exit not recorded';
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

type JobState = 'running' | 'ok' | 'failed' | 'ended';
type WaitState = 'waiting' | 'settled' | 'timed-out' | 'unknown' | 'unreadable';

const WAIT_STATES: Record<WaitVerdict, WaitState> = {
  settled: 'settled',
  timed_out: 'timed-out',
  silent: 'unknown',
  unreadable: 'unreadable',
};

/// A job exits a pass only on a clean code. A killed job has no code, which the
/// frozen shape uses to mean exactly that, and it is never a pass.
///
/// With no exit at all the answer is whose turn this is. A job dies with the
/// turn that started it and the exit lives only on the live frame, so a card
/// rebuilt from storage has no exit to show and no job left to run: saying it
/// is running claims a process that ended before the page was even loaded.
function jobState(exited: JobExited | undefined, live: boolean): JobState {
  if (!exited) return live ? 'running' : 'ended';
  return exited.exit_code === SUCCESSFUL_EXIT_CODE ? 'ok' : 'failed';
}

function jobTitle(exited: JobExited | undefined, live: boolean): string {
  if (!exited) return live ? RUNNING_LABEL : ENDED_LABEL;
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

function Fact({
  label,
  children,
  mono = false,
  wide = false,
}: {
  label: string;
  children: ReactNode;
  mono?: boolean;
  wide?: boolean;
}) {
  return (
    <div className={`job-card-fact${wide ? ' job-card-fact--wide' : ''}`}>
      <dt>{label}</dt>
      <dd className={mono ? 'job-card-mono' : undefined}>{children}</dd>
    </div>
  );
}

/// A path is told apart by its end, so the row keeps the file name and lets
/// the ellipsis eat the directories; a click shows the whole thing.
function Path({ value }: { value: string }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <button
      type="button"
      className={`job-card-path${expanded ? ' job-card-path--expanded' : ''}`}
      title={value}
      aria-expanded={expanded}
      onClick={() => setExpanded((open) => !open)}
    >
      <bdi>{value}</bdi>
    </button>
  );
}

function Job({ job, exited, live }: { job?: JobStarted; exited?: JobExited; live: boolean }) {
  const state = jobState(exited, live);

  return (
    <div className={`job-card job-card--${state}`} data-testid="job-card">
      <p className="job-card-header">
        <span className="job-card-status" aria-hidden="true" />
        <span className="job-card-title">{jobTitle(exited, live)}</span>
      </p>
      <dl className="job-card-meta">
        <Fact label="Job" mono>
          {job?.id ?? exited?.id}
        </Fact>
        {job ? (
          <Fact label="Process" mono>
            {job.pid}
          </Fact>
        ) : null}
        {job ? (
          <Fact label="Log" mono wide>
            <Path value={job.log_path} />
          </Fact>
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
          <Fact label="Id" mono>
            {waiting.id}
          </Fact>
          {waiting.reference ? (
            <Fact label="Reference" mono>
              {waiting.reference}
            </Fact>
          ) : null}
          {settled ? null : (
            <Fact label="Until" wide>
              <Deadline deadline={waiting.deadline} />
            </Fact>
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
///
/// `live` is the turn that started this work still being written, which is the
/// only turn a job can still be running in. It defaults to false because that
/// is what a stored message is, and a caller that forgets it should understate
/// rather than claim a process that is gone.
export function JobCard({
  call,
  live = false,
}: {
  call: Pick<ToolCallRecord, 'job' | 'exited' | 'waiting' | 'settled'>;
  live?: boolean;
}) {
  const { job, exited, waiting, settled } = call;

  return (
    <>
      {job || exited ? <Job job={job} exited={exited} live={live} /> : null}
      {waiting || settled ? <Wait waiting={waiting} settled={settled} /> : null}
    </>
  );
}
