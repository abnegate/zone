import { Button, Checkbox, Modal } from '@zone/ui';
import { useId, useState } from 'react';
import type { GitHubApp } from '../../../github';

interface GitHubRemoveDialogProps {
  app: GitHubApp;
  onConfirm: (uninstall: boolean) => Promise<void>;
  onClose: () => void;
}

export function GitHubRemoveDialog({ app, onConfirm, onClose }: GitHubRemoveDialogProps) {
  const uninstallId = useId();
  const [uninstall, setUninstall] = useState(true);
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const confirm = async () => {
    setRemoving(true);
    setError(null);
    try {
      await onConfirm(uninstall);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : 'Failed to remove the GitHub account');
      setRemoving(false);
    }
  };

  return (
    <Modal
      isOpen
      onClose={removing ? undefined : onClose}
      title={`Remove ${app.owner_login}?`}
      size="sm"
    >
      <div className="confirm-modal github-remove">
        <p>Sources using this account stop working until they are reconnected.</p>
        <Checkbox
          id={uninstallId}
          label="Also uninstall from GitHub"
          checked={uninstall}
          disabled={removing}
          onCheckedChange={setUninstall}
        />
        {error && (
          <div className="modal-error" role="alert">
            {error}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={removing} onClick={onClose}>
            Cancel
          </Button>
          <Button variant="danger" loading={removing} onClick={confirm}>
            Remove account
          </Button>
        </div>
      </div>
    </Modal>
  );
}
