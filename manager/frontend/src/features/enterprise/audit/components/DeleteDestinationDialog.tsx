import { Button, Modal } from '@zone/ui';
import { useDeleteAuditDestination } from '../hooks';
import { CANCEL_LABEL, DELETE_CONFIRM_LABEL, DELETE_TITLE } from '../labels';
import { deleteMessage, refusalMessage } from '../messages';
import type { AuditScope, DestinationResponse } from '../types';

interface DeleteDestinationDialogProps {
  scope: AuditScope;
  destination: DestinationResponse;
  onClose(): void;
}

export function DeleteDestinationDialog({
  scope,
  destination,
  onClose,
}: DeleteDestinationDialogProps) {
  const { remove, pending, refusal, error } = useDeleteAuditDestination(scope);
  const failure = refusal ? refusalMessage(refusal.code) : error;

  return (
    <Modal isOpen onClose={pending ? undefined : onClose} title={DELETE_TITLE} size="sm">
      <div className="audit-dialog">
        <p>{deleteMessage(destination.name)}</p>
        {failure && (
          <div className="modal-error" role="alert">
            {failure}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={pending} onClick={onClose}>
            {CANCEL_LABEL}
          </Button>
          <Button
            variant="danger"
            loading={pending}
            onClick={() => remove(destination.id, onClose)}
          >
            {DELETE_CONFIRM_LABEL}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
