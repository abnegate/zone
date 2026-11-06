import { PERMISSIONS } from '../../../shared/types/permissions';
import './ModelAccessNotice.css';

export const MODEL_ACCESS_NOTICE = `Installing or training models on this server requires the ${PERMISSIONS.MODELS.CREATE} permission. Ask a server admin for access.`;

export default function ModelAccessNotice() {
  return (
    <p className="model-access-notice" role="note">
      {MODEL_ACCESS_NOTICE}
    </p>
  );
}
