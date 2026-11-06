import { Button, Modal } from '@zone/ui';

interface ResetDialogProps {
  open: boolean;
  title: string;
  removals: string[];
  confirmLabel: string;
  onConfirm: () => void;
  onClose: () => void;
}

export default function ResetDialog({
  open,
  title,
  removals,
  confirmLabel,
  onConfirm,
  onClose,
}: ResetDialogProps) {
  return (
    <Modal isOpen={open} onClose={onClose} title={title} size="sm">
      <div className="confirm-modal">
        <p>This removes from this workspace:</p>
        <ul>
          {removals.map((removal) => (
            <li key={removal}>{removal}</li>
          ))}
        </ul>
        <p>The workspace then follows the organization.</p>
        <div className="modal-actions">
          <Button type="button" variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="button" variant="danger" onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
