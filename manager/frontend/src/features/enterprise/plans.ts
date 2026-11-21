import { Edition, Plan } from './schemas';
import type { Edition as EditionValue, Plan as PlanValue } from './types';

const INCLUDED: Readonly<Record<EditionValue, readonly PlanValue[]>> = {
  [Edition.Community]: [],
  [Edition.Team]: [Plan.Team],
  [Edition.Enterprise]: [Plan.Team, Plan.Enterprise],
};

const PLAN_LABELS: Readonly<Record<PlanValue, string>> = {
  [Plan.Team]: 'Team',
  [Plan.Enterprise]: 'Enterprise',
};

export function includes(edition: EditionValue, plan: PlanValue): boolean {
  return INCLUDED[edition].includes(plan);
}

export function planLabel(plan: PlanValue): string {
  return PLAN_LABELS[plan];
}

export function availableOn(plan: PlanValue): string {
  return `Available on ${planLabel(plan)}`;
}
