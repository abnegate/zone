import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { PERMISSIONS } from '../../../shared/types/permissions';
import { useAuth } from '../../auth/context';
import type { SettingsAccess } from './sections';
import { useWorkspaceRole } from './useWorkspaceRole';

export interface SettingsAccessState {
  access: SettingsAccess;
  loading: boolean;
  resolvingWorkspaceRole: boolean;
}

export function useSettingsAccess(): SettingsAccessState {
  const { currentOrganization, currentWorkspace, loading, resolvingRole } = useWorkspace();
  const { role: workspaceRole, resolving: resolvingWorkspaceRole } = useWorkspaceRole();
  const { hasPermission } = useAuth();

  return {
    access: {
      organization: currentOrganization !== null,
      organizationRole: currentOrganization?.role,
      workspace: currentWorkspace !== null,
      workspaceRole,
      installsModels: hasPermission(PERMISSIONS.MODELS.READ),
    },
    loading: loading || resolvingRole,
    resolvingWorkspaceRole,
  };
}
