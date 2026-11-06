import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { type SettingsAccess, SettingsGroup, SettingsSection } from '../navigation/sections';
import {
  AuditLogsSection,
  BillingSection,
  DevicesSection,
  OrgMembersSection,
} from '../organization/components';
import {
  ConnectDevicesSection,
  HostFoldersPanel,
  WorkspaceMembersSection,
} from '../workspace/components';

interface SettingsContentProps {
  group: SettingsGroup;
  section: SettingsSection;
  access: SettingsAccess;
}

export default function SettingsContent({ group, section }: SettingsContentProps) {
  const { currentOrganization, currentWorkspace } = useWorkspace();
  const orgId = currentOrganization?.id ?? null;
  const workspaceId = currentWorkspace?.id ?? null;

  if (group === SettingsGroup.Workspace) {
    if (section === SettingsSection.Folders) {
      return <HostFoldersPanel workspaceId={workspaceId} orgId={orgId} />;
    }
    if (section === SettingsSection.Members && workspaceId !== null && orgId !== null) {
      return <WorkspaceMembersSection workspaceId={workspaceId} orgId={orgId} />;
    }
    return null;
  }

  if (group === SettingsGroup.Organization && orgId !== null) {
    switch (section) {
      case SettingsSection.Members:
        return <OrgMembersSection orgId={orgId} />;
      case SettingsSection.Devices:
        return <DevicesSection orgId={orgId} />;
      case SettingsSection.Billing:
        return <BillingSection orgId={orgId} />;
      case SettingsSection.Audit:
        return <AuditLogsSection orgId={orgId} />;
      default:
        return null;
    }
  }

  if (group === SettingsGroup.Server && section === SettingsSection.Connect) {
    return <ConnectDevicesSection />;
  }

  return null;
}
