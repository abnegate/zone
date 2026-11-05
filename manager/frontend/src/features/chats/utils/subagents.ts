import type { ChatSubagent, StepStatus, SubagentStatus } from '../subagentTypes';
import type { TaskTint } from './tasks';

const SECOND = 1_000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const MILLISECOND_DIGITS = 3;
const FRACTIONAL_SECONDS = /T[^.]*\.(\d+)/;
const UNNAMED = 'Agent';

const STATUS_LABELS: Record<SubagentStatus, string> = {
  running: 'Running',
  completed: 'Completed',
  failed: 'Failed',
  stopped: 'Stopped',
};

const STATUS_TINTS: Record<SubagentStatus, TaskTint> = {
  running: 'info',
  completed: 'success',
  failed: 'destructive',
  stopped: 'neutral',
};

const STEP_LABELS: Record<StepStatus, string> = {
  running: 'Running',
  succeeded: 'Succeeded',
  failed: 'Failed',
};

/**
 * The moment a server timestamp names, in milliseconds with the microseconds
 * Postgres keeps as a fraction. Comparing strings would order `+02:00` against
 * `Z`, or `.5` against `.123456`, by their characters rather than their time.
 */
export function instant(stamp: string | null): number {
  if (stamp === null) return Number.NaN;
  const milliseconds = Date.parse(stamp);
  if (Number.isNaN(milliseconds)) return milliseconds;
  const digits = FRACTIONAL_SECONDS.exec(stamp)?.[1] ?? '';
  const beyond = digits.slice(MILLISECOND_DIGITS);
  return beyond ? milliseconds + Number(`0.${beyond}`) : milliseconds;
}

function newer(candidate: ChatSubagent, held: ChatSubagent): boolean {
  return !(instant(held.updated_at) > instant(candidate.updated_at));
}

function startOrder(stamp: string): number {
  const time = instant(stamp);
  return Number.isNaN(time) ? Number.POSITIVE_INFINITY : time;
}

function byStart(left: ChatSubagent, right: ChatSubagent): number {
  const difference = startOrder(left.started_at) - startOrder(right.started_at);
  if (difference !== 0 && !Number.isNaN(difference)) return difference;
  if (left.id === right.id) return 0;
  return left.id < right.id ? -1 : 1;
}

/** Folds rows into a list, keeping whichever copy of each was written last, in start order. */
export function mergeSubagents(list: ChatSubagent[], loaded: ChatSubagent[]): ChatSubagent[] {
  const merged = new Map(list.map((subagent) => [subagent.id, subagent]));
  let changed = false;
  for (const incoming of loaded) {
    const held = merged.get(incoming.id);
    if (held && (held === incoming || !newer(incoming, held))) continue;
    merged.set(incoming.id, incoming);
    changed = true;
  }
  return changed ? [...merged.values()].sort(byStart) : list;
}

export function mergeSubagent(list: ChatSubagent[], incoming: ChatSubagent): ChatSubagent[] {
  return mergeSubagents(list, [incoming]);
}

export function startedIn(list: ChatSubagent[], messageId: string): ChatSubagent[] {
  return list.filter((subagent) => subagent.message_id === messageId);
}

/** What the agent was asked to do, or a plain name when it was given no description. */
export function subagentName(subagent: ChatSubagent): string {
  return subagent.description.trim() || UNNAMED;
}

export function subagentActive(subagent: ChatSubagent): boolean {
  return subagent.status === 'running';
}

export function subagentStatusLabel(status: SubagentStatus): string {
  return STATUS_LABELS[status];
}

export function subagentStatusTint(status: SubagentStatus): TaskTint {
  return STATUS_TINTS[status];
}

export function stepStatusLabel(status: StepStatus): string {
  return STEP_LABELS[status];
}

/**
 * How long the subagent has run, in milliseconds: live while it runs, and as
 * the agent reported it once it stops, falling back to the stored times.
 */
export function subagentDuration(subagent: ChatSubagent, now: number): number | null {
  const started = instant(subagent.started_at);
  if (subagentActive(subagent)) {
    const elapsed = Number.isNaN(started) ? null : Math.max(0, now - started);
    if (elapsed === null) return subagent.duration_ms;
    return Math.max(elapsed, subagent.duration_ms ?? 0);
  }
  if (subagent.duration_ms !== null) return subagent.duration_ms;
  const finished = instant(subagent.finished_at);
  if (Number.isNaN(started) || Number.isNaN(finished)) return null;
  return Math.max(0, finished - started);
}

/** `'42s'`, `'3m 05s'` or `'1h 02m'`. */
export function formatDuration(milliseconds: number): string {
  const total = Number.isFinite(milliseconds) ? Math.max(0, Math.floor(milliseconds)) : 0;
  const hours = Math.floor(total / HOUR);
  const minutes = Math.floor((total % HOUR) / MINUTE);
  const seconds = Math.floor((total % MINUTE) / SECOND);
  if (hours > 0) return `${hours}h ${String(minutes).padStart(2, '0')}m`;
  if (minutes > 0) return `${minutes}m ${String(seconds).padStart(2, '0')}s`;
  return `${seconds}s`;
}
