import { Button, Modal } from '@zone/ui';
import { useRevokeScimToken } from '../hooks';
import { CANCEL_LABEL, REVOKE_CONFIRM, revokeTitle, revokeWarning } from '../labels';
import { refusalMessage } from '../messages';
import type { TokenResponse } from '../types';

interface RevokeScimTokenDialogProps {
  organizationId: string;
  token: TokenResponse;
  onRevoked: (token: TokenResponse) => void;
  onClose: () => void;
}

export function RevokeScimTokenDialog({
  organizationId,
  token,
  onRevoked,
  onClose,
}: RevokeScimTokenDialogProps) {
  const { revoke, pending, refusal, error } = useRevokeScimToken(organizationId);
  const failure = refusal ? refusalMessage(refusal.code) : error;

  return (
    <Modal isOpen onClose={pending ? undefined : onClose} title={revokeTitle(token.name)} size="sm">
      <div className="scim-dialog">
        <p>{revokeWarning(token.name)}</p>
        {failure && (
          <div className="modal-error" role="alert">
            {failure}
          </div>
        )}
        <div className="modal-actions">
          <Button type="button" variant="ghost" disabled={pending} onClick={onClose}>
            {CANCEL_LABEL}
          </Button>
          <Button
            type="button"
            variant="danger"
            loading={pending}
            onClick={() => revoke(token.id, () => onRevoked(token))}
          >
            {REVOKE_CONFIRM}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
