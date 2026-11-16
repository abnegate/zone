import {
  type StatusTint,
  statusLabel,
  statusTint,
  TASK_STATUSES,
  type TaskStatus,
} from '../tasks/status';
import type { ErrorCategory } from './types';

export const Tone = {
  Neutral: 'neutral',
  Info: 'info',
  Warning: 'warning',
  Destructive: 'destructive',
  Accent: 'accent',
  Success: 'success',
} as const satisfies Record<string, StatusTint>;

export type Tone = StatusTint;

export const NO_SOURCE = 'No source';

export const NO_MODEL = 'Default model';

const CATEGORY_LABELS: Record<ErrorCategory, string> = {
  build: 'Build',
  test_failure: 'Test failures',
  dependency: 'Dependencies',
  permission: 'Permissions',
  timeout: 'Timeouts',
  rate_limit: 'Rate limits',
  network: 'Network',
  merge_conflict: 'Merge conflicts',
  configuration: 'Configuration',
  refusal: 'Refusals',
  unknown: 'Unclassified',
};

const ORIGIN_LABELS: Readonly<Record<string, string>> = {
  member: 'Members',
  reminder: 'Reminders',
  automation: 'Automations',
};

const QUALITY_LABELS: Readonly<Record<string, string>> = {
  poor: 'Poor',
  mixed: 'Mixed',
  solid: 'Solid',
  exemplary: 'Exemplary',
};

const QUALITY_TONES: Readonly<Record<string, Tone>> = {
  poor: Tone.Destructive,
  mixed: Tone.Warning,
  solid: Tone.Info,
  exemplary: Tone.Success,
};

function isTaskStatus(name: string): name is TaskStatus {
  return (TASK_STATUSES as readonly string[]).includes(name);
}

export function humanize(name: string): string {
  const spaced = name.replaceAll('_', ' ');
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

export function categoryLabel(category: ErrorCategory): string {
  return CATEGORY_LABELS[category];
}

export function taskStatusLabel(name: string): string {
  return isTaskStatus(name) ? statusLabel(name) : humanize(name);
}

export function taskStatusTone(name: string): Tone {
  return isTaskStatus(name) ? statusTint(name) : Tone.Neutral;
}

export function originLabel(name: string): string {
  return ORIGIN_LABELS[name] ?? humanize(name);
}

export function qualityLabel(name: string): string {
  return QUALITY_LABELS[name] ?? humanize(name);
}

export function qualityTone(name: string): Tone {
  return QUALITY_TONES[name] ?? Tone.Neutral;
}
