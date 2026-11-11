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

const STAGE_BY_ID: Record<StageId, Stage> = {
  planned: { id: 'planned', label: 'Planned', target: 'created' },
  in_progress: { id: 'in_progress', label: 'In progress', target: 'in_progress' },
  blocked: { id: 'blocked', label: 'Blocked', target: 'blocked' },
  paused: { id: 'paused', label: 'Paused', target: 'paused' },
  review: { id: 'review', label: 'In review', target: 'review' },
  complete: { id: 'complete', label: 'Completed', target: 'complete' },
  cancelled: { id: 'cancelled', label: 'Cancelled', target: 'cancelled' },
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
