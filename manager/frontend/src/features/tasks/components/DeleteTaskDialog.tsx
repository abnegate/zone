import { Button, Modal } from '@zone/ui';
import { useState } from 'react';
import type { Task } from '../types';

const FALLBACK_ERROR = 'Failed to delete the task';

interface DeleteTaskDialogProps {
  task: Task;
  onConfirm: () => Promise<void>;
  onClose: () => void;
}

export function DeleteTaskDialog({ task, onConfirm, onClose }: DeleteTaskDialogProps) {
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const confirm = async () => {
    setDeleting(true);
    setError(null);
    try {
      await onConfirm();
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : FALLBACK_ERROR);
      setDeleting(false);
    }
  };

  return (
    <Modal
      isOpen
      onClose={deleting ? undefined : onClose}
      title={`Delete "${task.title}"?`}
      size="sm"
    >
      <p>This cannot be undone.</p>
      {error && (
        <div className="modal-error" role="alert">
          {error}
        </div>
      )}
      <div className="modal-actions">
        <Button variant="ghost" disabled={deleting} onClick={onClose}>
          Cancel
        </Button>
        <Button variant="danger" loading={deleting} onClick={confirm}>
          Delete task
        </Button>
      </div>
    </Modal>
  );
}
