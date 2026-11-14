import type { StatusTint } from '../../tasks/status';
import type { ChatSubagent, StepStatus, SubagentStatus, SubagentStep } from '../subagentTypes';

const SECOND = 1_000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const MILLISECOND_DIGITS = 3;
const FRACTIONAL_SECONDS = /T[^.]*\.(\d+)/;
const UNNAMED = 'Agent';
const LABEL_PREFIX = `${UNNAMED}: `;

const STATUS_LABELS: Record<SubagentStatus, string> = {
  running: 'Running',
  completed: 'Completed',
  failed: 'Failed',
  stopped: 'Stopped',
};

const STATUS_TINTS: Record<SubagentStatus, StatusTint> = {
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

function supersedes(candidate: ChatSubagent, held: ChatSubagent): boolean {
  return candidate.revision >= held.revision;
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

/**
 * Folds rows into a list in start order, keeping the copy of each with the
 * higher revision; on a tie the incoming copy wins.
 */
export function mergeSubagents(list: ChatSubagent[], loaded: ChatSubagent[]): ChatSubagent[] {
  const merged = new Map(list.map((subagent) => [subagent.id, subagent]));
  let changed = false;
  for (const incoming of loaded) {
    const held = merged.get(incoming.id);
    if (held && (held === incoming || !supersedes(incoming, held))) continue;
    merged.set(incoming.id, incoming);
    changed = true;
  }
  return changed ? [...merged.values()].sort(byStart) : list;
}

export function mergeSubagent(list: ChatSubagent[], incoming: ChatSubagent): ChatSubagent[] {
  return mergeSubagents(list, [incoming]);
}

export function withoutSubagents(list: ChatSubagent[], ids: ReadonlySet<string>): ChatSubagent[] {
  const kept = list.filter((subagent) => !ids.has(subagent.id));
  return kept.length === list.length ? list : kept;
}

/** The ids of the listed agents that are not among those kept. */
export function absentSubagents(
  list: readonly ChatSubagent[],
  kept: ReadonlySet<string>
): Set<string> {
  return new Set(list.filter((subagent) => !kept.has(subagent.id)).map((subagent) => subagent.id));
}

/** Each message's subagents, in start order, keyed by the message whose turn started them. */
export function groupByMessage(list: readonly ChatSubagent[]): Map<string, ChatSubagent[]> {
  const grouped = new Map<string, ChatSubagent[]>();
  for (const subagent of list) {
    if (subagent.message_id === null) continue;
    const started = grouped.get(subagent.message_id);
    if (started) started.push(subagent);
    else grouped.set(subagent.message_id, [subagent]);
  }
  return grouped;
}

function settled(step: SubagentStep): boolean {
  return step.status !== 'running';
}

/**
 * Applies step updates over a list by step id, appending new steps in arrival
 * order. A settled step never goes back to running, so an update older than
 * the copy held cannot undo it.
 */
export function mergeSteps(list: SubagentStep[], updates: readonly SubagentStep[]): SubagentStep[] {
  if (updates.length === 0) return list;
  const merged = [...list];
  const positions = new Map(merged.map((step, index) => [step.id, index]));
  let changed = false;
  for (const update of updates) {
    const position = positions.get(update.id);
    if (position === undefined) {
      positions.set(update.id, merged.length);
      merged.push(update);
      changed = true;
      continue;
    }
    const held = merged[position];
    if (held === update || (settled(held) && !settled(update))) continue;
    merged[position] = update;
    changed = true;
  }
  return changed ? merged : list;
}

/** What the agent was asked to do, or a plain name when it was given no description. */
export function subagentName(subagent: ChatSubagent): string {
  return subagent.description.trim() || UNNAMED;
}

/** `Agent: <description>`, or just `Agent` when it was given no description. */
export function subagentLabel(subagent: ChatSubagent): string {
  const description = subagent.description.trim();
  return description ? `${LABEL_PREFIX}${description}` : UNNAMED;
}

export function subagentActive(subagent: ChatSubagent): boolean {
  return subagent.status === 'running';
}

export function subagentStatusLabel(status: SubagentStatus): string {
  return STATUS_LABELS[status];
}

export function subagentStatusTint(status: SubagentStatus): StatusTint {
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
