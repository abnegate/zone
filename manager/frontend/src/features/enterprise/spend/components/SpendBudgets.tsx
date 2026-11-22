import { useState } from 'react';
import { SectionCard } from '../../../analytics/components/SectionCard';
import { type SpendTargets, useBudgetMutations } from '../hooks';
import { BUDGETS_TITLE } from '../labels';
import { budgetsDescription } from '../messages';
import type { Budget, BudgetsResponse } from '../schemas';
import { BudgetEditor } from './BudgetEditor';
import { BudgetList } from './BudgetList';
import { DeleteBudgetDialog } from './DeleteBudgetDialog';

const NEW_BUDGET = 'new';

interface SpendBudgetsProps {
  organizationId: string;
  response: BudgetsResponse;
  targets: SpendTargets;
  editable: boolean;
}

export function SpendBudgets({ organizationId, response, targets, editable }: SpendBudgetsProps) {
  const mutations = useBudgetMutations(organizationId);
  const [editing, setEditing] = useState<Budget | null>(null);
  const [deleting, setDeleting] = useState<Budget | null>(null);

  return (
    <SectionCard title={BUDGETS_TITLE} description={budgetsDescription(response.month)} wide>
      <div className="spend-budget-panel">
        <BudgetList
          budgets={response.budgets}
          editable={editable}
          editing={editing?.id ?? null}
          onEdit={(budget) => {
            mutations.create.reset();
            mutations.update.reset();
            setEditing(budget);
          }}
          onDelete={setDeleting}
        />
        <BudgetEditor
          key={editing?.id ?? NEW_BUDGET}
          budget={editing}
          editable={editable}
          workspaces={targets.workspaces}
          people={targets.people}
          mutations={mutations}
          onDone={() => setEditing(null)}
        />
      </div>
      {deleting && (
        <DeleteBudgetDialog
          budget={deleting}
          mutation={mutations.remove}
          onClose={() => {
            if (editing?.id === deleting.id) setEditing(null);
            setDeleting(null);
          }}
        />
      )}
    </SectionCard>
  );
}
