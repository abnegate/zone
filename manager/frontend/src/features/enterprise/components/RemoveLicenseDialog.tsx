import { Button, Modal } from '@zone/ui';
import { useRemoveLicense } from '../hooks';

interface RemoveLicenseDialogProps {
  onClose: () => void;
}

export function RemoveLicenseDialog({ onClose }: RemoveLicenseDialogProps) {
  const { remove, pending, error } = useRemoveLicense();

  return (
    <Modal isOpen onClose={pending ? undefined : onClose} title="Remove the license?" size="sm">
      <div className="license-remove">
        <p>
          This server goes back to the Community edition and configuring Enterprise features stops.
          Sign-in and existing policies keep working.
        </p>
        {error && (
          <div className="modal-error" role="alert">
            {error}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={pending} onClick={onClose}>
            Cancel
          </Button>
          <Button variant="danger" loading={pending} onClick={() => remove(onClose)}>
            Remove license
          </Button>
        </div>
      </div>
    </Modal>
  );
}
