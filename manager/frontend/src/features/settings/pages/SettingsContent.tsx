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
  findSection,
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
  organizationId: string | null
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
          orgId={organizationId}
          readOnly={!allowsWorkspace(access.workspaceRole, WORKSPACE_EDIT_ROLE.folders)}
        />
      );
    case SettingsSection.Members:
      return workspaceId !== null && organizationId !== null ? (
        <WorkspaceMembersSection workspaceId={workspaceId} orgId={organizationId} />
      ) : null;
    default:
      return null;
  }
}

function organizationContent(
  section: SettingsSection,
  access: SettingsAccess,
  organizationId: string
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
      return <DevicesSection orgId={organizationId} />;
    case SettingsSection.Billing:
      return <BillingSection orgId={organizationId} />;
    case SettingsSection.Audit:
      return <AuditLogsSection orgId={organizationId} />;
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
  const organizationId = currentOrganization?.id ?? null;
  const workspaceId = currentWorkspace?.id ?? null;

  if (!findSection(group, section)?.visible(access)) {
    return null;
  }

  switch (group) {
    case SettingsGroup.Workspace:
      return workspaceContent(section, access, workspaceId, organizationId);
    case SettingsGroup.Organization:
      return organizationId === null ? null : organizationContent(section, access, organizationId);
    case SettingsGroup.Server:
      return serverContent(section);
    case SettingsGroup.You:
      return youContent(section);
  }
}
