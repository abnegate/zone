import { Button, Modal } from '@zone/ui';
import type { Task } from '../types';

interface DiscardChangesDialogProps {
  task: Task;
  onDiscard: () => void;
  onClose: () => void;
}

export function DiscardChangesDialog({ task, onDiscard, onClose }: DiscardChangesDialogProps) {
  return (
    <Modal isOpen onClose={onClose} title={`Discard your changes to ${task.title}?`} size="sm">
      <p>Your edits to this task have not been saved.</p>
      <div className="modal-actions">
        <Button variant="ghost" onClick={onClose}>
          Keep editing
        </Button>
        <Button variant="danger" onClick={onDiscard}>
          Discard changes
        </Button>
      </div>
    </Modal>
  );
}
