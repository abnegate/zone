import { Badge, Button, Modal } from '@zone/ui';
import { useEffect, useRef, useState } from 'react';
import { tasksApi } from '../../../api/tasks';
import { ActionReceipts } from '../../chats/components';
import type { Waiting } from '../../chats/types';
import { QuestionPrompt } from '../components';
import type { RunStatus, Task, TaskRun, TaskRunLog } from '../types';

const WAITING_PHASE = 'waiting';
const WAITING_LABEL = 'Waiting for';
const ANSWER_ACTIVITY = 'Waiting for an answer';
const WAITING_STATUS = 'Waiting';
const WAITING_FOR_YOU_STATUS = 'Waiting for you';
const DEADLINE_PASSED_LABEL = 'Deadline passed';

const KIND_JOB = 'job';
const KIND_TASK_RUN = 'task_run';
const KIND_CHECK = 'check';

const ACTIVITIES: Record<string, string> = {
  thinking: 'Thinking',
  acting: 'Using tools',
  observing: 'Reviewing results',
  waiting: 'Waiting',
  responding: 'Writing response',
  complete: 'Completed',
  error: 'Failed',
};

const STATUSES: Record<RunStatus, string> = {
  pending: 'Queued',
  running: 'Running',
  waiting: WAITING_FOR_YOU_STATUS,
  completed: 'Completed',
  failed: 'Failed',
  cancelled: 'Cancelled',
};

/**
 * Whether the run is still this console's to watch.
 *
 * A waiting run counts: it has stopped at a question or a wait rather than
 * finished, and dropping it here would stop the poll at the very moment the
 * question arrives or the wait settles, leaving the reader looking at a run
 * that appears stalled with nothing to answer.
 */
function active(run: TaskRun): boolean {
  return run.status === 'pending' || run.status === 'running' || run.status === 'waiting';
}

/**
 * The wait a parked run is on when nobody is being asked anything. A question
 * on the same run wins: it needs the reader, and the wait needs no one.
 */
function wait(run: TaskRun): Waiting | undefined {
  if (run.status !== 'waiting' || run.pending_question) return undefined;
  return run.waiting_on ?? undefined;
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

function statusLabel(run: TaskRun): string {
  return wait(run) ? WAITING_STATUS : STATUSES[run.status];
}

/// The phase, read with what the run carries: a parked run reads as a question
/// or as a wait, never as an answer nobody was asked for.
function activity(run: TaskRun): string | null {
  const phase = run.current_phase;
  if (!phase) return null;
  if (phase === WAITING_PHASE) {
    if (run.pending_question) return ANSWER_ACTIVITY;
    const waiting = wait(run);
    if (waiting) return `${WAITING_LABEL} ${waitSubject(waiting)}`;
  }
  return ACTIVITIES[phase] ?? phase;
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

function Countdown({ deadline }: { deadline: string }) {
  const remaining = useRemaining(deadline);

  if (remaining === null) return null;
  return (
    <span role="timer" aria-label="Time remaining" data-testid="wait-countdown">
      {remaining > 0 ? formatRemaining(remaining) : DEADLINE_PASSED_LABEL}
    </span>
  );
}

export function TaskExecutionView({ task, onClose }: { task: Task; onClose: () => void }) {
  const [run, setRun] = useState<TaskRun | null>(null);
  const [logs, setLogs] = useState<TaskRunLog[]>([]);
  const [loading, setLoading] = useState(true);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [monitoring, setMonitoring] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const controller = useRef<AbortController | null>(null);
  const busy = useRef(false);
  const submitted = useRef<TaskRun | null>(null);
  const known = useRef(new Set<string>());

  // biome-ignore lint/correctness/useExhaustiveDependencies: revision explicitly refreshes the monitor after a start or user retry
  useEffect(() => {
    const request = new AbortController();
    controller.current = request;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let failures = 0;
    const refresh = async (current: TaskRun): Promise<void> => {
      try {
        const [snapshot, entries] = await Promise.all([
          tasksApi.getTaskRun(task.id, current.id, request.signal),
          tasksApi.getTaskRunLogs(task.id, current.id, request.signal),
        ]);
        if (request.signal.aborted) return;
        setRun(snapshot);
        setLogs(entries);
        setMonitoring(null);
        failures = 0;
        if (active(snapshot)) timer = setTimeout(() => void refresh(snapshot), 2000);
      } catch (failure) {
        if (request.signal.aborted) return;
        setMonitoring(failure instanceof Error ? failure.message : 'Unable to refresh this run');
        failures += 1;
        timer = setTimeout(() => void refresh(current), Math.min(2000 * 2 ** failures, 30000));
      }
    };
    const restore = async (): Promise<void> => {
      setLoading(true);
      try {
        const runs = await tasksApi.getTaskRuns(task.id, request.signal);
        if (request.signal.aborted) return;
        known.current = new Set(runs.map((entry) => entry.id));
        const latest =
          runs.find(active) ??
          submitted.current ??
          [...runs].sort((left, right) =>
            (right.started_at ?? '').localeCompare(left.started_at ?? '')
          )[0];
        setMonitoring(null);
        setRun(latest ?? null);
        if (latest) await refresh(latest);
      } catch (failure) {
        if (!request.signal.aborted)
          setMonitoring(
            failure instanceof Error ? failure.message : 'Unable to load previous runs'
          );
      } finally {
        if (!request.signal.aborted) setLoading(false);
      }
    };
    void restore();
    return () => {
      request.abort();
      clearTimeout(timer);
    };
  }, [task.id, revision]);

  const start = async (): Promise<void> => {
    if (busy.current || loading || monitoring || (run && active(run))) return;
    const request = controller.current;
    if (!request || request.signal.aborted) return;
    busy.current = true;
    setStarting(true);
    setError(null);
    try {
      const result = await tasksApi.runTask(task.id, request.signal);
      if (request.signal.aborted) return;
      submitted.current = result;
      setRun(result);
      setLogs([]);
      setRevision((value) => value + 1);
    } catch (failure) {
      if (request.signal.aborted) return;
      setError(failure instanceof Error ? failure.message : 'Unable to start this task');
      try {
        const runs = await tasksApi.getTaskRuns(task.id, request.signal);
        if (request.signal.aborted) return;
        const accepted = runs.find((entry) => active(entry) || !known.current.has(entry.id));
        if (accepted) {
          submitted.current = accepted;
          setRun(accepted);
          setError(null);
          setRevision((value) => value + 1);
        }
      } catch {
        if (!request.signal.aborted)
          setMonitoring('The start result is unknown. Refresh status before trying again.');
      }
    } finally {
      busy.current = false;
      if (!request.signal.aborted) setStarting(false);
    }
  };

  const running = !!run && active(run);
  const waiting = run ? wait(run) : undefined;
  const phase = run ? activity(run) : null;
  const status = starting
    ? 'Starting'
    : loading
      ? 'Loading run'
      : run
        ? statusLabel(run)
        : 'Ready to run';
  return (
    <Modal
      isOpen
      onClose={onClose}
      title={task.title}
      className="task-execution-modal"
      aria-describedby="execution-description"
    >
      <p id="execution-description" className="execution-description">
        {task.description}
      </p>
      <div className="execution-summary" aria-live="polite">
        <Badge
          variant={
            run?.status === 'failed'
              ? 'destructive'
              : run?.status === 'completed'
                ? 'success'
                : run?.status === 'waiting'
                  ? 'warning'
                  : 'secondary'
          }
        >
          {status}
        </Badge>
        {phase && <span>{phase}</span>}
        {waiting && (
          <span>
            until <time dateTime={waiting.deadline}>{formatDeadline(waiting.deadline)}</time>
          </span>
        )}
        {waiting && <Countdown deadline={waiting.deadline} />}
      </div>
      {!run && !loading && !starting && !error && (
        <p className="execution-hint">
          Start this task to see its activity and execution logs here.
        </p>
      )}
      {error && (
        <div className="execution-notice" role="alert">
          <strong>Could not start task</strong>
          <p>{error}</p>
        </div>
      )}
      {monitoring && (
        <div className="execution-notice" role="alert">
          <strong>Monitoring unavailable</strong>
          <p>{monitoring}</p>
          <p>The run status could not be verified. Closing this window does not stop a run.</p>
          <Button variant="outline" size="sm" onClick={() => setRevision((value) => value + 1)}>
            Refresh status
          </Button>
        </div>
      )}
      {run?.status === 'failed' && (
        <div className="execution-notice" role="alert">
          <strong>Task failed</strong>
          <p>{run.error_message || 'The worker could not complete this task.'}</p>
        </div>
      )}
      {run?.status === 'completed' && (
        <p className="execution-hint">Task completed successfully.</p>
      )}
      {run?.status === 'waiting' && run.pending_question && (
        <QuestionPrompt run={run} onAnswered={() => setRevision((value) => value + 1)} />
      )}
      {run && (
        <section className="execution-logs" aria-label="Execution logs">
          <h3>Execution Logs</h3>
          {logs.length ? (
            <div className="logs-container">
              {logs.map((log) => {
                // A receipt carries the actor, target, outcome and a link to
                // the item. Its log message is only "Workspace action receipt",
                // so showing the line alone throws the record away.
                const receipt = log.metadata?.action_receipt;
                return (
                  <div key={log.id} className="log-entry">
                    <span className="log-phase">{ACTIVITIES[log.phase] ?? log.phase}</span>
                    <span className="log-details">
                      {log.agent_type} · {log.level}
                    </span>
                    {receipt ? (
                      <ActionReceipts receipts={[receipt]} />
                    ) : (
                      <span className="log-message">{log.message}</span>
                    )}
                  </div>
                );
              })}
            </div>
          ) : (
            <p className="execution-hint">
              {running ? 'Waiting for worker output…' : 'No logs were recorded for this run.'}
            </p>
          )}
        </section>
      )}
      <footer className="execution-controls">
        <p className="execution-hint">
          {running || starting
            ? 'You can close this window. The task continues in the background.'
            : 'Runs use this task’s configured model and sources.'}
        </p>
        {!running && (
          <Button onClick={() => void start()} disabled={loading || starting || !!monitoring}>
            {starting ? 'Starting…' : error ? 'Try Again' : run ? 'Run Again' : 'Start Execution'}
          </Button>
        )}
      </footer>
    </Modal>
  );
}
