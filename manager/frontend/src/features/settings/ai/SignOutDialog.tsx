import { Button, Modal } from '@zone/ui';

interface SignOutDialogProps {
  open: boolean;
  name: string;
  busy: boolean;
  onConfirm: () => void;
  onClose: () => void;
}

export function SignOutDialog({ open, name, busy, onConfirm, onClose }: SignOutDialogProps) {
  return (
    <Modal
      isOpen={open}
      onClose={busy ? undefined : onClose}
      title={`Sign out of ${name}?`}
      size="sm"
    >
      <p>This signs {name} out for every workspace in this organization.</p>
      <div className="modal-actions">
        <Button variant="secondary" onClick={onClose} disabled={busy}>
          Cancel
        </Button>
        <Button variant="danger" onClick={onConfirm} loading={busy}>
          Sign out
        </Button>
      </div>
    </Modal>
  );
}
