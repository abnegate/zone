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
  const {
    role: workspaceRole,
    permissions: workspacePermissions,
    resolving: resolvingWorkspaceRole,
  } = useWorkspaceRole();
  const { hasPermission, user } = useAuth();

  return {
    access: {
      organization: currentOrganization !== null,
      organizationRole: currentOrganization?.role,
      organizationPermissions: currentOrganization?.permissions,
      workspace: currentWorkspace !== null,
      workspaceRole,
      workspacePermissions,
      installsModels: hasPermission(PERMISSIONS.MODELS.READ),
      readsSources: hasPermission(PERMISSIONS.SOURCES.READ),
      administersServer: user?.is_admin === true,
    },
    loading: loading || resolvingRole,
    resolvingWorkspaceRole,
  };
}
