import { Button, Modal } from '@zone/ui';
import { useRotateScimToken } from '../hooks';
import { CANCEL_LABEL, ROTATE_CONFIRM, rotateTitle, rotateWarning } from '../labels';
import { refusalMessage } from '../messages';
import type { IssuedResponse, TokenResponse } from '../types';

interface RotateScimTokenDialogProps {
  organizationId: string;
  token: TokenResponse;
  onIssued: (issued: IssuedResponse) => void;
  onClose: () => void;
}

export function RotateScimTokenDialog({
  organizationId,
  token,
  onIssued,
  onClose,
}: RotateScimTokenDialogProps) {
  const { rotate, pending, refusal, error } = useRotateScimToken(organizationId);
  const failure = refusal ? refusalMessage(refusal.code) : error;

  return (
    <Modal isOpen onClose={pending ? undefined : onClose} title={rotateTitle(token.name)} size="sm">
      <div className="scim-dialog">
        <p>{rotateWarning(token.name)}</p>
        {failure && (
          <div className="modal-error" role="alert">
            {failure}
          </div>
        )}
        <div className="modal-actions">
          <Button type="button" variant="ghost" disabled={pending} onClick={onClose}>
            {CANCEL_LABEL}
          </Button>
          <Button type="button" loading={pending} onClick={() => rotate(token.id, onIssued)}>
            {ROTATE_CONFIRM}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
