import type { Tint } from '../../github';
import type { CheckState, CheckSummary, PullRequest } from '../types';
import { READINESS_LABELS, READINESS_TINTS } from './grouping';

export const CHECK_TINTS: Readonly<Record<CheckState, Tint>> = {
  success: 'success',
  failure: 'destructive',
  pending: 'info',
  none: 'neutral',
  unknown: 'neutral',
};

export const CHECK_LABELS: Readonly<Record<CheckState, string>> = {
  success: 'Passed',
  failure: 'Failed',
  pending: 'Running',
  none: 'No checks',
  unknown: 'Unknown',
};

export const DRAFT_LABEL = 'Draft';

export const INCOMPLETE_LABEL = 'incomplete';

const SUMMARY_SEPARATOR = ' · ';

export interface CardBadge {
  label: string;
  tint: Tint;
}

export function summarizeChecks(checks: CheckSummary): string {
  const { total, passed, failed, running, queued, in_progress: inProgress } = checks.counts;
  if (total === 0) return CHECK_LABELS.none;
  const active = running + queued + inProgress;
  const parts = [
    ...(failed > 0 ? [`${failed} failed`] : []),
    ...(active > 0 ? [`${active} running`] : []),
    `${passed}/${total} passed`,
    ...(checks.complete ? [] : [INCOMPLETE_LABEL]),
  ];
  return parts.join(SUMMARY_SEPARATOR);
}

export function cardBadge(pullRequest: Pick<PullRequest, 'draft' | 'readiness'>): CardBadge {
  if (pullRequest.draft) return { label: DRAFT_LABEL, tint: 'neutral' };
  return {
    label: READINESS_LABELS[pullRequest.readiness],
    tint: READINESS_TINTS[pullRequest.readiness],
  };
}
