import { getSourceLabel } from '../sources/config';
import { SourceTypeSchema } from '../sources/schemas';
import { statusLabel, statusTint, TASK_STATUSES, type TaskStatus } from '../tasks/status';
import { Tone } from './Tone';
import type { ErrorCategory } from './types';

export const NO_SOURCE = 'No source';

export const NO_MODEL = 'Model not recorded';

const CATEGORIES: Record<ErrorCategory, string> = {
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

const ORIGINS: Readonly<Record<string, string>> = {
  member: 'Members',
  reminder: 'Reminders',
  automation: 'Automations',
};

const QUALITIES: Readonly<Record<string, string>> = {
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

export const AnalyticsLabel = {
  category(category: ErrorCategory): string {
    return CATEGORIES[category];
  },

  taskStatus(name: string): string {
    return isTaskStatus(name) ? statusLabel(name) : name;
  },

  taskStatusTone(name: string): Tone {
    return isTaskStatus(name) ? statusTint(name) : Tone.Neutral;
  },

  origin(name: string): string {
    return ORIGINS[name] ?? name;
  },

  originTone(): Tone {
    return Tone.Accent;
  },

  quality(name: string): string {
    return QUALITIES[name] ?? name;
  },

  qualityTone(name: string): Tone {
    return QUALITY_TONES[name] ?? Tone.Neutral;
  },

  sourceType(name: string): string {
    const parsed = SourceTypeSchema.safeParse(name);
    return parsed.success ? getSourceLabel(parsed.data) : name;
  },
} as const;
