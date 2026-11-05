import type { ChatTask, ChatTaskRun, ChatTaskRunStatus, ChatTaskStatus } from '../types';

export type TaskTint = 'neutral' | 'info' | 'warning' | 'destructive' | 'success';

const STATUS_LABELS: Record<ChatTaskStatus, string> = {
  created: 'Created',
  queued: 'Queued',
  in_progress: 'In progress',
  review: 'In review',
  complete: 'Complete',
  blocked: 'Blocked',
};

const STATUS_TINTS: Record<ChatTaskStatus, TaskTint> = {
  created: 'neutral',
  queued: 'info',
  in_progress: 'info',
  review: 'warning',
  complete: 'success',
  blocked: 'destructive',
};

const RUN_LABELS: Record<ChatTaskRunStatus, string> = {
  running: 'Running',
  waiting: 'Waiting',
  completed: 'Run completed',
  failed: 'Run failed',
  cancelled: 'Run cancelled',
};

const PHASES: Record<string, string> = {
  thinking: 'thinking',
  acting: 'using tools',
  observing: 'reviewing results',
  waiting: 'waiting',
  responding: 'writing response',
};

const ACTIVE_RUN_STATUSES: ReadonlySet<ChatTaskRunStatus> = new Set(['running', 'waiting']);
const ACTIVE_TASK_STATUSES: ReadonlySet<ChatTaskStatus> = new Set(['queued', 'in_progress']);

const GITHUB_PULL_REQUEST = /^https:\/\/github\.com\/([^/]+)\/([^/]+)\/pull\/(\d+)/;

export function taskStatusLabel(status: ChatTaskStatus): string {
  return STATUS_LABELS[status];
}

export function taskStatusTint(status: ChatTaskStatus): TaskTint {
  return STATUS_TINTS[status];
}

export function runActive(run: ChatTaskRun | null): boolean {
  return run !== null && ACTIVE_RUN_STATUSES.has(run.status);
}

/** Whether the task may still change on its own, so the panel keeps reading it. */
export function taskActive(task: ChatTask): boolean {
  return task.run ? runActive(task.run) : ACTIVE_TASK_STATUSES.has(task.status);
}

/** What the task's run is doing, or how it ended; nothing for a task that never ran. */
export function runReading(run: ChatTaskRun | null): string | null {
  if (!run) return null;
  const label = RUN_LABELS[run.status];
  const phase = run.phase ? PHASES[run.phase] : undefined;
  return runActive(run) && phase && phase !== run.status ? `${label}, ${phase}` : label;
}

/** The run's progress while it is still going, clamped to a percentage. */
export function runProgress(run: ChatTaskRun | null): number | null {
  if (!run || !runActive(run) || run.progress === null) return null;
  return Math.min(100, Math.max(0, Math.round(run.progress)));
}

export function publicationName(url: string): string {
  const match = GITHUB_PULL_REQUEST.exec(url);
  return match ? `${match[2]} #${match[3]}` : 'Pull request';
}
