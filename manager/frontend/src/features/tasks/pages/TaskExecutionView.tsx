import { Badge, Button, Modal } from '@zone/ui';
import { useEffect, useRef, useState } from 'react';
import { tasksApi } from '../../../api/tasks';
import type { Task, TaskRun, TaskRunLog } from '../types';

const ACTIVITIES: Record<string, string> = {
  thinking: 'Thinking',
  acting: 'Using tools',
  observing: 'Reviewing results',
  responding: 'Writing response',
  complete: 'Completed',
  error: 'Failed',
};

function active(run: TaskRun): boolean {
  return run.status === 'pending' || run.status === 'running';
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
  const status = starting
    ? 'Starting'
    : loading
      ? 'Loading run'
      : run
        ? {
            pending: 'Queued',
            running: 'Running',
            completed: 'Completed',
            failed: 'Failed',
            cancelled: 'Cancelled',
          }[run.status]
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
                : 'secondary'
          }
        >
          {status}
        </Badge>
        {run?.current_phase && <span>{ACTIVITIES[run.current_phase] ?? run.current_phase}</span>}
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
      {run && (
        <section className="execution-logs" aria-label="Execution logs">
          <h3>Execution Logs</h3>
          {logs.length ? (
            <div className="logs-container">
              {logs.map((log) => (
                <div key={log.id} className="log-entry">
                  <span className="log-phase">{ACTIVITIES[log.phase] ?? log.phase}</span>
                  <span className="log-details">
                    {log.agent_type} · {log.level}
                  </span>
                  <span className="log-message">{log.message}</span>
                </div>
              ))}
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
