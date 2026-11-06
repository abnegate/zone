import { PERMISSIONS } from '../../../shared/types/permissions';
import { useAuth } from '../../auth/context';

export interface ModelAccess {
  canInstall: boolean;
  canRemove: boolean;
}

export function useModelAccess(): ModelAccess {
  const { hasPermission } = useAuth();
  return {
    canInstall: hasPermission(PERMISSIONS.MODELS.CREATE),
    canRemove: hasPermission(PERMISSIONS.MODELS.DELETE),
  };
}
