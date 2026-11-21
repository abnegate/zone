import { monthLabel } from './calendar';
import {
  AUTOMATION_LABEL,
  DELETED_WORKSPACE,
  ORGANIZATION_TARGET,
  RANGE_TOO_LONG,
  UNKNOWN_PERSON,
  UNKNOWN_PROVIDER,
  UNKNOWN_WORKSPACE,
} from './labels';
import { money } from './money';
import { MAXIMUM_DAYS, RangeProblem, type RangeProblem as RangeProblemValue } from './range';
import {
  type Budget,
  BudgetScope,
  type BudgetScope as BudgetScopeValue,
  BudgetState,
  type BudgetState as BudgetStateValue,
  SpendCode,
  type SpendRefusalCode,
  UncountedReason,
  type UncountedReason as UncountedReasonValue,
  type UserUsage,
  type WorkspaceUsage,
} from './schemas';

const UNKNOWN_PROVIDER_NAME = 'unknown';

const SUMMARY_SEPARATOR = ' · ';

const REFUSALS: Readonly<Record<SpendRefusalCode, string>> = {
  [SpendCode.LicenseRequired]:
    'Changing budgets needs a Zone Team license that is active or in its grace period.',
  [SpendCode.BodyInvalid]: "Zone couldn't read this change. Reload the page and try again.",
  [SpendCode.RangeInvalid]: `Choose real dates, with the start on or before the end and ${MAXIMUM_DAYS} days at most.`,
  [SpendCode.LimitInvalid]: 'Enter a monthly limit above $0, like 50 or 12.50.',
  [SpendCode.ThresholdInvalid]: 'Enter a warning threshold from 1 to 100 percent.',
  [SpendCode.ScopeInvalid]: 'Choose a workspace or person that belongs to this organization.',
  [SpendCode.BudgetExists]: 'That already has a budget. Edit the existing one instead.',
};

const UNCOUNTED: Readonly<Record<UncountedReasonValue, string>> = {
  [UncountedReason.CodingAgent]:
    "Coding-agent subscriptions: chats on Claude Code or Codex run on someone's own sign-in, not through Zone's LiteLLM.",
  [UncountedReason.DirectProvider]:
    'Direct provider keys: requests sent straight to OpenAI or Anthropic with a saved key.',
  [UncountedReason.ExternalEndpoint]:
    "Other endpoints: self-hosted or OpenAI-compatible servers other than Zone's LiteLLM.",
  [UncountedReason.SavedKey]:
    "Workspace keys saved for this server's LiteLLM: those requests carry the saved key, so Zone can't tell who sent them.",
};

const SCOPES: Readonly<Record<BudgetScopeValue, string>> = {
  [BudgetScope.Organization]: 'Organization',
  [BudgetScope.Workspace]: 'Workspace',
  [BudgetScope.User]: 'Person',
};

const STATES: Readonly<Record<BudgetStateValue, string>> = {
  [BudgetState.Under]: 'On track',
  [BudgetState.Warning]: 'Past its warning',
  [BudgetState.Exhausted]: 'Used up',
};

export const BudgetField = {
  Limit: 'limit',
  Threshold: 'threshold',
  Target: 'target',
} as const;

export type BudgetField = (typeof BudgetField)[keyof typeof BudgetField];

const ERROR_FIELDS: Readonly<Partial<Record<SpendRefusalCode, BudgetField>>> = {
  [SpendCode.LimitInvalid]: BudgetField.Limit,
  [SpendCode.ThresholdInvalid]: BudgetField.Threshold,
  [SpendCode.ScopeInvalid]: BudgetField.Target,
};

export function refusalMessage(code: SpendRefusalCode): string {
  return REFUSALS[code];
}

export function rangeMessage(problem: RangeProblemValue): string {
  switch (problem) {
    case RangeProblem.Invalid:
      return refusalMessage(SpendCode.RangeInvalid);
    case RangeProblem.TooLong:
      return RANGE_TOO_LONG;
  }
}

export function refusalField(code: SpendRefusalCode): BudgetField | null {
  return ERROR_FIELDS[code] ?? null;
}

export function uncountedLine(reason: UncountedReasonValue): string {
  return UNCOUNTED[reason];
}

export function scopeLabel(scope: BudgetScopeValue): string {
  return SCOPES[scope];
}

export function stateLabel(state: BudgetStateValue): string {
  return STATES[state];
}

export function budgetTarget(budget: Budget): string {
  switch (budget.scope) {
    case BudgetScope.Organization:
      return ORGANIZATION_TARGET;
    case BudgetScope.Workspace:
      return budget.workspace_name ?? UNKNOWN_WORKSPACE;
    case BudgetScope.User:
      return budget.user_name || budget.user_email || UNKNOWN_PERSON;
  }
}

export function budgetSpent(budget: Budget): string {
  return `${money(budget.spent_microdollars)} of ${money(budget.limit_microdollars)}`;
}

export function budgetSummary(budget: Budget): string {
  return [budgetSpent(budget), `warns at ${budget.threshold_percent}%`].join(SUMMARY_SEPARATOR);
}

export function meterLabel(budget: Budget): string {
  return `Spend against the budget for ${budgetTarget(budget)}`;
}

export function actionLabel(action: string, target: string): string {
  return `${action} the budget for ${target}`;
}

export function budgetsDescription(month: string): string {
  return `Spend in ${monthLabel(month)}. Budgets reset on the 1st (UTC). Hard limits are checked as spend is counted, so a request already running can go a little over.`;
}

export function deleteMessage(target: string): string {
  return `Delete the budget for ${target}? Spend keeps being counted, but nobody is warned or stopped any more.`;
}

export function workspaceName(usage: Pick<WorkspaceUsage, 'id' | 'name'>): string {
  return usage.id === null ? DELETED_WORKSPACE : (usage.name ?? DELETED_WORKSPACE);
}

export function personName(usage: Pick<UserUsage, 'id' | 'name' | 'email' | 'automated'>): string {
  if (usage.automated) return AUTOMATION_LABEL;
  if (usage.id === null) return UNKNOWN_PERSON;
  return usage.name || usage.email || UNKNOWN_PERSON;
}

export function providerName(name: string): string {
  return name === UNKNOWN_PROVIDER_NAME ? UNKNOWN_PROVIDER : name;
}
