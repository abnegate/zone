import { Button, Modal } from '@zone/ui';
import { useState } from 'react';
import { messageOf } from './failure';

export const REMOVE_FAILED = 'Failed to remove the MCP server';

interface McpRemoveDialogProps {
  name: string;
  onConfirm: () => Promise<void>;
  onClose: () => void;
}

export function McpRemoveDialog({ name, onConfirm, onClose }: McpRemoveDialogProps) {
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const confirm = async () => {
    setRemoving(true);
    setError(null);
    try {
      await onConfirm();
    } catch (failure) {
      setError(messageOf(failure, REMOVE_FAILED));
      setRemoving(false);
    }
  };

  return (
    <Modal isOpen onClose={removing ? undefined : onClose} title={`Remove ${name}?`} size="sm">
      <div className="mcp-remove">
        <p>Chats and tasks lose its tools.</p>
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
            Remove server
          </Button>
        </div>
      </div>
    </Modal>
  );
}
