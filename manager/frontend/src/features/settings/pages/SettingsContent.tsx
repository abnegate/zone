import type { ReactNode } from 'react';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { SessionsSection } from '../../auth/components';
import {
  AdministersServerProvider,
  AuditExportSection,
  LicenseSection,
  OrganizationPolicySection,
  organizationScope,
  Permission,
  RolesSection,
  SERVER_SCOPE,
  SpendPage,
  SsoSection,
  WorkspacePolicySection,
} from '../../enterprise';
import { SourcesSection } from '../../sources/components';
import { McpServersSection, WorkspaceMcpServersSection } from '../mcp';
import {
  grantsOrganization,
  grantsWorkspace,
  type OrganizationPermission,
  WORKSPACE_EDIT_PERMISSION,
  type WorkspacePermission,
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
  DevicesSection,
  GitHubSection,
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

function inOrganization(access: SettingsAccess, name: OrganizationPermission): boolean {
  return grantsOrganization(access.organizationPermissions, access.organizationRole, name);
}

function inWorkspace(access: SettingsAccess, name: WorkspacePermission): boolean {
  return grantsWorkspace(access.workspacePermissions, access.workspaceRole, name);
}

function workspaceContent(
  section: SettingsSection,
  access: SettingsAccess,
  workspaceId: string | null,
  organizationId: string | null
): ReactNode {
  switch (section) {
    case SettingsSection.General:
      return <GeneralSection editable={inWorkspace(access, WORKSPACE_EDIT_PERMISSION.theme)} />;
    case SettingsSection.Ai:
      return (
        <WorkspaceAiSection
          editable={inWorkspace(access, WORKSPACE_EDIT_PERMISSION.ai)}
          organizationDefaultsPath={
            inOrganization(access, Permission.AiSettingsRead)
              ? settingsPath(SettingsGroup.Organization, SettingsSection.Ai)
              : undefined
          }
        />
      );
    case SettingsSection.AgentPolicy:
      return workspaceId !== null && organizationId !== null ? (
        <WorkspacePolicySection
          organizationId={organizationId}
          workspaceId={workspaceId}
          organizationPolicyPath={
            inOrganization(access, Permission.PoliciesUpdate)
              ? settingsPath(SettingsGroup.Organization, SettingsSection.AgentPolicy)
              : undefined
          }
        />
      ) : null;
    case SettingsSection.Folders:
      return (
        <HostFoldersPanel
          workspaceId={workspaceId}
          orgId={organizationId}
          readOnly={!inWorkspace(access, WORKSPACE_EDIT_PERMISSION.folders)}
        />
      );
    case SettingsSection.Sources:
      return <SourcesSection readOnly={!inWorkspace(access, WORKSPACE_EDIT_PERMISSION.sources)} />;
    case SettingsSection.Members:
      return workspaceId !== null && organizationId !== null ? (
        <WorkspaceMembersSection workspaceId={workspaceId} orgId={organizationId} />
      ) : null;
    case SettingsSection.Mcp:
      return workspaceId !== null && organizationId !== null ? (
        <WorkspaceMcpServersSection
          organizationId={organizationId}
          workspaceId={workspaceId}
          editable={inWorkspace(access, WORKSPACE_EDIT_PERMISSION.mcp)}
        />
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
      return (
        <OrganizationAiSection editable={inOrganization(access, Permission.AiSettingsUpdate)} />
      );
    case SettingsSection.AgentPolicy:
      return <OrganizationPolicySection organizationId={organizationId} />;
    case SettingsSection.Members:
      return (
        <OrganizationMembersSection
          invitations={{
            read: inOrganization(access, Permission.InvitationsRead),
            create: inOrganization(access, Permission.InvitationsCreate),
            revoke: inOrganization(access, Permission.InvitationsDelete),
            role: access.organizationRole,
            permissions: access.organizationPermissions,
          }}
        />
      );
    case SettingsSection.Roles:
      return <RolesSection organizationId={organizationId} />;
    case SettingsSection.GitHub:
      return (
        <GitHubSection
          organizationId={organizationId}
          canManage={inOrganization(access, Permission.GitHubUpdate)}
        />
      );
    case SettingsSection.Mcp:
      return (
        <McpServersSection
          organizationId={organizationId}
          canManage={inOrganization(access, Permission.McpServersUpdate)}
        />
      );
    case SettingsSection.Devices:
      return (
        <DevicesSection
          orgId={organizationId}
          editable={inOrganization(access, Permission.DevicesUpdate)}
        />
      );
    case SettingsSection.Audit:
      return <AuditLogsSection orgId={organizationId} />;
    case SettingsSection.AuditExport:
      return <AuditExportSection scope={organizationScope(organizationId)} />;
    case SettingsSection.Spend:
      return <SpendPage organizationId={organizationId} />;
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
    case SettingsSection.License:
      return <LicenseSection />;
    case SettingsSection.Sso:
      return <SsoSection />;
    case SettingsSection.AuditExport:
      return <AuditExportSection scope={SERVER_SCOPE} />;
    default:
      return null;
  }
}

function youContent(section: SettingsSection): ReactNode {
  return section === SettingsSection.Sessions ? <SessionsSection /> : null;
}

function groupContent(
  group: SettingsGroup,
  section: SettingsSection,
  access: SettingsAccess,
  workspaceId: string | null,
  organizationId: string | null
): ReactNode {
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

export default function SettingsContent({ group, section, access }: SettingsContentProps) {
  const { currentOrganization, currentWorkspace } = useWorkspace();
  const organizationId = currentOrganization?.id ?? null;
  const workspaceId = currentWorkspace?.id ?? null;

  if (!findSection(group, section)?.visible(access)) {
    return null;
  }

  return (
    <AdministersServerProvider value={access.administersServer}>
      {groupContent(group, section, access, workspaceId, organizationId)}
    </AdministersServerProvider>
  );
}
