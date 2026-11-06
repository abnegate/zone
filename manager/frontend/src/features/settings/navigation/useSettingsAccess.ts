import { useWorkspace } from '../../../shared/context/WorkspaceContext';
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

  return {
    access: {
      organization: currentOrganization !== null,
      organizationRole: currentOrganization?.role,
      workspace: currentWorkspace !== null,
      workspaceRole,
    },
    loading: loading || resolvingRole,
    resolvingWorkspaceRole,
  };
}
