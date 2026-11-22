import { Badge, type BadgeProps, Button } from '@zone/ui';
import { DELETE_LABEL, EDIT_LABEL, HARD_BADGE, NO_BUDGETS, SOFT_BADGE } from '../labels';
import {
  actionLabel,
  budgetSpent,
  budgetSummary,
  budgetTarget,
  meterLabel,
  scopeLabel,
  stateLabel,
} from '../messages';
import { type Budget, BudgetState, type BudgetState as BudgetStateValue } from '../schemas';

const PERCENT = 100;

const BADGES: Readonly<Record<BudgetStateValue, BadgeProps['variant']>> = {
  [BudgetState.Under]: 'success',
  [BudgetState.Warning]: 'warning',
  [BudgetState.Exhausted]: 'destructive',
};

function used(budget: Budget): number {
  return Math.min(
    PERCENT,
    Math.floor((budget.spent_microdollars * PERCENT) / budget.limit_microdollars)
  );
}

interface BudgetListProps {
  budgets: readonly Budget[];
  editable: boolean;
  editing: string | null;
  onEdit(budget: Budget): void;
  onDelete(budget: Budget): void;
}

export function BudgetList({ budgets, editable, editing, onEdit, onDelete }: BudgetListProps) {
  if (budgets.length === 0) {
    return <p className="analytics-quiet">{NO_BUDGETS}</p>;
  }
  return (
    <ul className="spend-budgets">
      {budgets.map((budget) => {
        const target = budgetTarget(budget);
        const percent = used(budget);
        return (
          <li key={budget.id} className="spend-budget" data-state={budget.state}>
            <div className="spend-budget-header">
              <span className="spend-budget-name">{target}</span>
              <span className="spend-budget-scope">{scopeLabel(budget.scope)}</span>
              <Badge variant={BADGES[budget.state]}>{stateLabel(budget.state)}</Badge>
              <span className="spend-budget-kind">{budget.hard ? HARD_BADGE : SOFT_BADGE}</span>
            </div>
            <div
              className="spend-meter"
              role="meter"
              aria-label={meterLabel(budget)}
              aria-valuemin={0}
              aria-valuemax={PERCENT}
              aria-valuenow={percent}
              aria-valuetext={budgetSpent(budget)}
            >
              <span className="spend-meter-fill" style={{ inlineSize: `${percent}%` }} />
              <span
                className="spend-meter-threshold"
                style={{ insetInlineStart: `${budget.threshold_percent}%` }}
              />
            </div>
            <div className="spend-budget-footer">
              <p className="spend-budget-summary">{budgetSummary(budget)}</p>
              {editable && (
                <div className="spend-budget-actions">
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    aria-label={actionLabel(EDIT_LABEL, target)}
                    aria-pressed={editing === budget.id}
                    onClick={() => onEdit(budget)}
                  >
                    {EDIT_LABEL}
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    aria-label={actionLabel(DELETE_LABEL, target)}
                    onClick={() => onDelete(budget)}
                  >
                    {DELETE_LABEL}
                  </Button>
                </div>
              )}
            </div>
          </li>
        );
      })}
    </ul>
  );
}
