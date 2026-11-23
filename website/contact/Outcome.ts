import type { Submission } from './Submission.ts';

export const OutcomeKind = {
  Accepted: 'accepted',
  Trapped: 'trapped',
  Invalid: 'invalid',
} as const;

export type Outcome =
  | { readonly kind: typeof OutcomeKind.Accepted; readonly submission: Submission }
  | { readonly kind: typeof OutcomeKind.Trapped; readonly redirect: URL }
  | { readonly kind: typeof OutcomeKind.Invalid; readonly problem: string };
