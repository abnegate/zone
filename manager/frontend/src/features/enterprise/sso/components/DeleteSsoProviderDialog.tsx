import { Button, Modal } from '@zone/ui';
import { useDeleteSsoProvider } from '../hooks';
import { refusalMessage } from '../messages';
import type { ProviderResponse } from '../types';

interface DeleteSsoProviderDialogProps {
  provider: ProviderResponse;
  onClose: () => void;
}

export function DeleteSsoProviderDialog({ provider, onClose }: DeleteSsoProviderDialogProps) {
  const { remove, pending, refusal, error } = useDeleteSsoProvider();
  const failure = refusal ? refusalMessage(refusal.code, refusal.message) : error;

  return (
    <Modal
      isOpen
      onClose={pending ? undefined : onClose}
      title={`Delete ${provider.display_name}?`}
      size="sm"
    >
      <div className="sso-dialog">
        <p>
          {provider.display_name} disappears from the sign-in page. People who signed in with it
          keep their Zone accounts, but can no longer sign in with {provider.display_name}.
        </p>
        {failure && (
          <div className="modal-error" role="alert">
            {failure}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={pending} onClick={onClose}>
            Cancel
          </Button>
          <Button variant="danger" loading={pending} onClick={() => remove(provider.id, onClose)}>
            Delete provider
          </Button>
        </div>
      </div>
    </Modal>
  );
}
