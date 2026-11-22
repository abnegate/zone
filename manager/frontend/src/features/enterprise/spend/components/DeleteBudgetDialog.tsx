import { Button, Modal } from '@zone/ui';
import type { BudgetMutations } from '../hooks';
import { CANCEL_LABEL, DELETE_CONFIRM_LABEL, DELETE_TITLE } from '../labels';
import { budgetTarget, deleteMessage, refusalMessage } from '../messages';
import type { Budget } from '../schemas';

interface DeleteBudgetDialogProps {
  budget: Budget;
  mutation: BudgetMutations['remove'];
  onClose(): void;
}

export function DeleteBudgetDialog({ budget, mutation, onClose }: DeleteBudgetDialogProps) {
  const failure = mutation.refusal ? refusalMessage(mutation.refusal.code) : mutation.error;
  const close = () => {
    mutation.reset();
    onClose();
  };

  return (
    <Modal isOpen onClose={mutation.pending ? undefined : close} title={DELETE_TITLE} size="sm">
      <div className="spend-dialog">
        <p>{deleteMessage(budgetTarget(budget))}</p>
        {failure && (
          <div className="modal-error" role="alert">
            {failure}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={mutation.pending} onClick={close}>
            {CANCEL_LABEL}
          </Button>
          <Button
            variant="danger"
            loading={mutation.pending}
            onClick={() => mutation.run(budget.id, close)}
          >
            {DELETE_CONFIRM_LABEL}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
