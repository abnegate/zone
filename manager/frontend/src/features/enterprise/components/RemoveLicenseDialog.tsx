import { Button, Modal } from '@zone/ui';
import { useLicenseChanging, useRemoveLicense } from '../hooks';

interface RemoveLicenseDialogProps {
  verified: boolean;
  onClose: () => void;
}

const VERIFIED_CONSEQUENCE = 'This server goes back to the Community edition.';

const UNVERIFIED_CONSEQUENCE =
  "This deletes the stored key this version of Zone can't verify. The server stays on the Community edition.";

export function RemoveLicenseDialog({ verified, onClose }: RemoveLicenseDialogProps) {
  const { remove, pending, error } = useRemoveLicense();
  const changing = useLicenseChanging();

  return (
    <Modal isOpen onClose={pending ? undefined : onClose} title="Remove the license?" size="sm">
      <div className="license-remove">
        <p>{verified ? VERIFIED_CONSEQUENCE : UNVERIFIED_CONSEQUENCE}</p>
        {error && (
          <div className="modal-error" role="alert">
            {error}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={pending} onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="danger"
            disabled={changing}
            loading={pending}
            onClick={() => remove(onClose)}
          >
            Remove license
          </Button>
        </div>
      </div>
    </Modal>
  );
}
