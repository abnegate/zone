import { Button, Modal } from '@zone/ui';
import { useState } from 'react';
import { useUpdateRunner } from '../hooks';
import {
  actionTitle,
  CANCEL_LABEL,
  CURRENT_WARNING,
  confirmLabel,
  consequence,
  failureText,
  RunnerAction,
  STATE_TARGETS,
  type StateAction,
} from '../messages';
import type { RunnerResponse } from '../types';
import { NoRunnerLeftDialog } from './NoRunnerLeftDialog';

interface RunnerStateDialogProps {
  runner: RunnerResponse;
  action: StateAction;
  onClose(): void;
}

export function RunnerStateDialog({ runner, action, onClose }: RunnerStateDialogProps) {
  const { update, pending, refusal, error } = useUpdateRunner();
  const [confirming, setConfirming] = useState(false);
  const state = STATE_TARGETS[action];
  const leftNoRunner = refusal?.leavesNoRunner ?? false;
  const failure = leftNoRunner ? null : failureText(refusal, error);

  const send = (confirm: boolean) =>
    update(runner.id, confirm ? { state, confirm } : { state }, onClose, (refused) => {
      if (refused.leavesNoRunner) setConfirming(true);
    });

  if (confirming) {
    return (
      <NoRunnerLeftDialog
        pending={pending}
        error={failure}
        onConfirm={() => send(true)}
        onClose={onClose}
      />
    );
  }

  return (
    <Modal
      isOpen
      onClose={pending ? undefined : onClose}
      title={actionTitle(action, runner.name)}
      size="sm"
    >
      <div className="runner-dialog">
        <p>{consequence(action, runner.name)}</p>
        {runner.current && action === RunnerAction.Revoke && <p>{CURRENT_WARNING}</p>}
        {failure && (
          <div className="modal-error" role="alert">
            {failure}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={pending} onClick={onClose}>
            {CANCEL_LABEL}
          </Button>
          <Button
            variant={action === RunnerAction.Revoke ? 'danger' : 'default'}
            loading={pending}
            onClick={() => send(false)}
          >
            {confirmLabel(action)}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
