import { Button, Modal } from '@zone/ui';
import { useDeleteRole } from '../hooks';
import { CANCEL_LABEL, DELETE_LABEL } from '../labels';
import { refusalMessage } from '../messages';
import type { CustomRole } from '../types';

interface DeleteRoleDialogProps {
  organizationId: string;
  role: CustomRole;
  onClose: () => void;
}

export function deleteWarning(name: string): string {
  return `${name} is removed from this organization. This can't be undone.`;
}

export function DeleteRoleDialog({ organizationId, role, onClose }: DeleteRoleDialogProps) {
  const { remove, pending, refusal, error } = useDeleteRole(organizationId);
  const failure = refusal ? refusalMessage(refusal.code, refusal.members) : error;

  return (
    <Modal isOpen onClose={pending ? undefined : onClose} title={`Delete ${role.name}?`} size="sm">
      <div className="roles-dialog">
        <p>{deleteWarning(role.name)}</p>
        {failure && (
          <div className="modal-error" role="alert">
            {failure}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={pending} onClick={onClose}>
            {CANCEL_LABEL}
          </Button>
          <Button variant="danger" loading={pending} onClick={() => remove(role.id, onClose)}>
            {DELETE_LABEL}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
