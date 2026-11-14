import type { TaskStatus } from '../../tasks/status';
import type { ChatTask, ChatTaskRun, ChatTaskRunStatus } from '../types';

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
const ACTIVE_TASK_STATUSES: ReadonlySet<TaskStatus> = new Set(['queued', 'in_progress']);

const GITHUB_PULL_REQUEST = /^https:\/\/github\.com\/([^/]+)\/([^/]+)\/pull\/(\d+)/;

export function runActive(run: ChatTaskRun | null): boolean {
  return run !== null && ACTIVE_RUN_STATUSES.has(run.status);
}

export function taskActive(task: ChatTask): boolean {
  return task.run ? runActive(task.run) : ACTIVE_TASK_STATUSES.has(task.status);
}

export function runReading(run: ChatTaskRun | null): string | null {
  if (!run) return null;
  const label = RUN_LABELS[run.status];
  const phase = run.phase ? PHASES[run.phase] : undefined;
  return runActive(run) && phase && phase !== run.status ? `${label}, ${phase}` : label;
}

export function runProgress(run: ChatTaskRun | null): number | null {
  if (!run || !runActive(run) || run.progress === null) return null;
  return Math.min(100, Math.max(0, Math.round(run.progress)));
}

export function publicationName(url: string): string {
  const match = GITHUB_PULL_REQUEST.exec(url);
  return match ? `${match[2]} #${match[3]}` : 'Pull request';
}
