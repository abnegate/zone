import type { StatusTint } from '../tasks/status';

export const Tone = {
  Neutral: 'neutral',
  Info: 'info',
  Warning: 'warning',
  Destructive: 'destructive',
  Accent: 'accent',
  Success: 'success',
} as const satisfies Record<string, StatusTint>;

export type Tone = StatusTint;
