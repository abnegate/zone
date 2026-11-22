import { Button, Modal } from '@zone/ui';
import {
  CANCEL_LABEL,
  NO_RUNNER_LEFT_CONFIRM,
  NO_RUNNER_LEFT_DETAIL,
  NO_RUNNER_LEFT_TITLE,
  refusalMessage,
} from '../messages';
import { FleetCode } from '../schemas';

interface NoRunnerLeftDialogProps {
  pending: boolean;
  error: string | null;
  onConfirm(): void;
  onClose(): void;
}

export function NoRunnerLeftDialog({
  pending,
  error,
  onConfirm,
  onClose,
}: NoRunnerLeftDialogProps) {
  return (
    <Modal isOpen onClose={pending ? undefined : onClose} title={NO_RUNNER_LEFT_TITLE} size="sm">
      <div className="runner-dialog">
        <p>{refusalMessage(FleetCode.NoRunnerLeft)}</p>
        <p>{NO_RUNNER_LEFT_DETAIL}</p>
        {error && (
          <div className="modal-error" role="alert">
            {error}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={pending} onClick={onClose}>
            {CANCEL_LABEL}
          </Button>
          <Button variant="danger" loading={pending} onClick={onConfirm}>
            {NO_RUNNER_LEFT_CONFIRM}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
