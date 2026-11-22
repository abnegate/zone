import { Button, Modal } from '@zone/ui';
import { useRemoveRunner } from '../hooks';
import {
  actionTitle,
  CANCEL_LABEL,
  confirmLabel,
  consequence,
  failureText,
  RunnerAction,
} from '../messages';
import type { RunnerResponse } from '../types';

interface RemoveRunnerDialogProps {
  runner: RunnerResponse;
  onClose(): void;
}

export function RemoveRunnerDialog({ runner, onClose }: RemoveRunnerDialogProps) {
  const { remove, pending, refusal, error } = useRemoveRunner();
  const failure = failureText(refusal, error);

  return (
    <Modal
      isOpen
      onClose={pending ? undefined : onClose}
      title={actionTitle(RunnerAction.Remove, runner.name)}
      size="sm"
    >
      <div className="runner-dialog">
        <p>{consequence(RunnerAction.Remove, runner.name)}</p>
        {failure && (
          <div className="modal-error" role="alert">
            {failure}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={pending} onClick={onClose}>
            {CANCEL_LABEL}
          </Button>
          <Button variant="danger" loading={pending} onClick={() => remove(runner.id, onClose)}>
            {confirmLabel(RunnerAction.Remove)}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
