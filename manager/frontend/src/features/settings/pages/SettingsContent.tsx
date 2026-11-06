import type { ReactNode } from 'react';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { SessionsSection } from '../../auth/components';
import {
  allowsOrganization,
  allowsWorkspace,
  ORGANIZATION_ADMIN_ROLE,
  WORKSPACE_EDIT_ROLE,
} from '../navigation/access';
import {
  type SettingsAccess,
  SettingsGroup,
  SettingsSection,
  settingsPath,
} from '../navigation/sections';
import {
  AuditLogsSection,
  BillingSection,
  DevicesSection,
  OrganizationAiSection,
  OrganizationMembersSection,
} from '../organization/components';
import { ConnectDevicesSection, FeaturesSection } from '../server/components';
import {
  GeneralSection,
  HostFoldersPanel,
  WorkspaceAiSection,
  WorkspaceMembersSection,
} from '../workspace/components';

interface SettingsContentProps {
  group: SettingsGroup;
  section: SettingsSection;
  access: SettingsAccess;
}

function workspaceContent(
  section: SettingsSection,
  access: SettingsAccess,
  workspaceId: string | null,
  orgId: string | null
): ReactNode {
  switch (section) {
    case SettingsSection.General:
      return (
        <GeneralSection
          editable={allowsWorkspace(access.workspaceRole, WORKSPACE_EDIT_ROLE.theme)}
        />
      );
    case SettingsSection.Ai:
      return (
        <WorkspaceAiSection
          editable={allowsWorkspace(access.workspaceRole, WORKSPACE_EDIT_ROLE.ai)}
          organizationDefaultsPath={
            allowsOrganization(access.organizationRole, ORGANIZATION_ADMIN_ROLE)
              ? settingsPath(SettingsGroup.Organization, SettingsSection.Ai)
              : undefined
          }
        />
      );
    case SettingsSection.Folders:
      return (
        <HostFoldersPanel
          workspaceId={workspaceId}
          orgId={orgId}
          readOnly={!allowsWorkspace(access.workspaceRole, WORKSPACE_EDIT_ROLE.folders)}
        />
      );
    case SettingsSection.Members:
      return workspaceId !== null && orgId !== null ? (
        <WorkspaceMembersSection workspaceId={workspaceId} orgId={orgId} />
      ) : null;
    default:
      return null;
  }
}

function organizationContent(
  section: SettingsSection,
  access: SettingsAccess,
  orgId: string
): ReactNode {
  switch (section) {
    case SettingsSection.Ai:
      return <OrganizationAiSection />;
    case SettingsSection.Members:
      return (
        <OrganizationMembersSection
          invitations={allowsOrganization(access.organizationRole, ORGANIZATION_ADMIN_ROLE)}
        />
      );
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

function serverContent(section: SettingsSection): ReactNode {
  switch (section) {
    case SettingsSection.Features:
      return <FeaturesSection />;
    case SettingsSection.Connect:
      return <ConnectDevicesSection />;
    default:
      return null;
  }
}

function youContent(section: SettingsSection): ReactNode {
  return section === SettingsSection.Sessions ? <SessionsSection /> : null;
}

export default function SettingsContent({ group, section, access }: SettingsContentProps) {
  const { currentOrganization, currentWorkspace } = useWorkspace();
  const orgId = currentOrganization?.id ?? null;
  const workspaceId = currentWorkspace?.id ?? null;

  switch (group) {
    case SettingsGroup.Workspace:
      return workspaceContent(section, access, workspaceId, orgId);
    case SettingsGroup.Organization:
      return orgId === null ? null : organizationContent(section, access, orgId);
    case SettingsGroup.Server:
      return serverContent(section);
    case SettingsGroup.You:
      return youContent(section);
  }
}
