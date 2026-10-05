import { Button, Modal } from '@zone/ui';
import type { Ref } from 'react';

interface SignOutDialogProps {
  open: boolean;
  name: string;
  account: string;
  onConfirm: () => void;
  onClose: () => void;
  ref?: Ref<HTMLDivElement>;
}

export function SignOutDialog({
  open,
  name,
  account,
  onConfirm,
  onClose,
  ref,
}: SignOutDialogProps) {
  return (
    <Modal ref={ref} isOpen={open} onClose={onClose} title={`Sign out ${account}?`} size="sm">
      <p>
        This signs {account} out of {name} for every workspace in this organization. Chats on it
        move to another account.
      </p>
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
