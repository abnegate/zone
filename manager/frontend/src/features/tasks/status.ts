import type { Stage } from './types';

export const TASK_STATUSES = [
  'created',
  'queued',
  'in_progress',
  'blocked',
  'paused',
  'review',
  'complete',
  'cancelled',
] as const;

export type TaskStatus = (typeof TASK_STATUSES)[number];

export type StatusTint = 'neutral' | 'info' | 'warning' | 'destructive' | 'accent' | 'success';

export const STAGE_IDS = [
  'planned',
  'in_progress',
  'blocked',
  'paused',
  'review',
  'complete',
  'cancelled',
] as const;

export type StageId = (typeof STAGE_IDS)[number];

const LABELS: Record<TaskStatus, string> = {
  created: 'Created',
  queued: 'Queued',
  in_progress: 'In progress',
  blocked: 'Blocked',
  paused: 'Paused',
  review: 'In review',
  complete: 'Completed',
  cancelled: 'Cancelled',
};

const TINTS: Record<TaskStatus, StatusTint> = {
  created: 'neutral',
  queued: 'info',
  in_progress: 'info',
  blocked: 'destructive',
  paused: 'neutral',
  review: 'warning',
  complete: 'success',
  cancelled: 'neutral',
};

const STAGE_OF_STATUS: Record<TaskStatus, StageId> = {
  created: 'planned',
  queued: 'planned',
  in_progress: 'in_progress',
  blocked: 'blocked',
  paused: 'paused',
  review: 'review',
  complete: 'complete',
  cancelled: 'cancelled',
};

function stage(id: StageId, label: string, target: TaskStatus): Stage {
  return {
    id,
    label,
    statuses: TASK_STATUSES.filter((status) => STAGE_OF_STATUS[status] === id),
    target,
    tint: TINTS[target],
  };
}

const STAGE_BY_ID: Record<StageId, Stage> = {
  planned: stage('planned', 'Planned', 'created'),
  in_progress: stage('in_progress', 'In progress', 'in_progress'),
  blocked: stage('blocked', 'Blocked', 'blocked'),
  paused: stage('paused', 'Paused', 'paused'),
  review: stage('review', 'In review', 'review'),
  complete: stage('complete', 'Completed', 'complete'),
  cancelled: stage('cancelled', 'Cancelled', 'cancelled'),
};

export const STAGES: readonly Stage[] = STAGE_IDS.map((id) => STAGE_BY_ID[id]);

export const STATUS_OPTIONS: readonly { value: TaskStatus; label: string }[] = TASK_STATUSES.map(
  (value) => ({ value, label: LABELS[value] })
);

export function statusLabel(status: TaskStatus): string {
  return LABELS[status];
}

export function statusTint(status: TaskStatus): StatusTint {
  return TINTS[status];
}

export function stageOf(status: TaskStatus): Stage {
  return STAGE_BY_ID[STAGE_OF_STATUS[status]];
}

export function groupByStage<T extends { status: TaskStatus }>(
  tasks: readonly T[]
): ReadonlyMap<StageId, T[]> {
  const groups = new Map<StageId, T[]>(STAGE_IDS.map((id) => [id, []]));
  for (const task of tasks) {
    groups.get(STAGE_OF_STATUS[task.status])?.push(task);
  }
  return groups;
}
