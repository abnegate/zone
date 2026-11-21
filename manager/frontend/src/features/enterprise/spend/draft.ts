import { BudgetField, refusalMessage } from './messages';
import { dollarsInput, parseDollars } from './money';
import {
  type Budget,
  BudgetScope,
  type BudgetScope as BudgetScopeValue,
  type CreateBudgetRequest,
  SpendCode,
  THRESHOLD_DEFAULT,
  THRESHOLD_MAXIMUM,
  THRESHOLD_MINIMUM,
  type UpdateBudgetRequest,
} from './schemas';

export interface BudgetDraft {
  scope: BudgetScopeValue;
  workspace: string;
  person: string;
  limit: string;
  threshold: string;
  hard: boolean;
}

export type BudgetErrors = Partial<Record<BudgetField, string>>;

const WHOLE_NUMBER = /^\d+$/;

export const EMPTY_DRAFT: BudgetDraft = {
  scope: BudgetScope.Organization,
  workspace: '',
  person: '',
  limit: '',
  threshold: String(THRESHOLD_DEFAULT),
  hard: false,
};

export function draftOf(budget: Budget): BudgetDraft {
  return {
    scope: budget.scope,
    workspace: budget.workspace_id ?? '',
    person: budget.user_id ?? '',
    limit: dollarsInput(budget.limit_microdollars),
    threshold: String(budget.threshold_percent),
    hard: budget.hard,
  };
}

function threshold(text: string): number | null {
  const trimmed = text.trim();
  if (!WHOLE_NUMBER.test(trimmed)) return null;
  const value = Number(trimmed);
  return value >= THRESHOLD_MINIMUM && value <= THRESHOLD_MAXIMUM ? value : null;
}

function limit(text: string): number | null {
  const value = parseDollars(text);
  return value !== null && value > 0 ? value : null;
}

function targetMissing(draft: BudgetDraft): boolean {
  switch (draft.scope) {
    case BudgetScope.Organization:
      return false;
    case BudgetScope.Workspace:
      return draft.workspace === '';
    case BudgetScope.User:
      return draft.person === '';
  }
}

export function errorsOf(draft: BudgetDraft): BudgetErrors {
  const errors: BudgetErrors = {};
  if (targetMissing(draft)) errors[BudgetField.Target] = refusalMessage(SpendCode.ScopeInvalid);
  if (limit(draft.limit) === null)
    errors[BudgetField.Limit] = refusalMessage(SpendCode.LimitInvalid);
  if (threshold(draft.threshold) === null) {
    errors[BudgetField.Threshold] = refusalMessage(SpendCode.ThresholdInvalid);
  }
  return errors;
}

export function createRequest(draft: BudgetDraft): CreateBudgetRequest | null {
  const microdollars = limit(draft.limit);
  const percent = threshold(draft.threshold);
  if (microdollars === null || percent === null || targetMissing(draft)) return null;
  return {
    scope: draft.scope,
    ...(draft.scope === BudgetScope.Workspace && { workspace_id: draft.workspace }),
    ...(draft.scope === BudgetScope.User && { user_id: draft.person }),
    limit_microdollars: microdollars,
    threshold_percent: percent,
    hard: draft.hard,
  };
}

export function updateRequest(budget: Budget, draft: BudgetDraft): UpdateBudgetRequest | null {
  const microdollars = limit(draft.limit);
  const percent = threshold(draft.threshold);
  if (microdollars === null || percent === null) return null;
  return {
    ...(microdollars !== budget.limit_microdollars && { limit_microdollars: microdollars }),
    ...(percent !== budget.threshold_percent && { threshold_percent: percent }),
    ...(draft.hard !== budget.hard && { hard: draft.hard }),
  };
}
