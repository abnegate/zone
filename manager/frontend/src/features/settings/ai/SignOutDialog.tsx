import { Button, Modal } from '@zone/ui';

interface SignOutDialogProps {
  open: boolean;
  name: string;
  onConfirm: () => void;
  onClose: () => void;
}

export function SignOutDialog({ open, name, onConfirm, onClose }: SignOutDialogProps) {
  return (
    <Modal isOpen={open} onClose={onClose} title={`Sign out of ${name}?`} size="sm">
      <p>This signs {name} out for every workspace in this organization.</p>
      <div className="modal-actions">
        <Button variant="secondary" onClick={onClose}>
          Cancel
        </Button>
        <Button variant="danger" onClick={onConfirm}>
          Sign out
        </Button>
      </div>
    </Modal>
  );
}
