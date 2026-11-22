import type { OrgRole, WorkspaceRole } from '../../../types';
import { Permission } from '../../enterprise/roles/schemas';
import { grantsOrganization, type OrganizationPermission } from './access';

export const SETTINGS_ROOT = '/settings';

export const SETTINGS_TITLE = 'Settings';

export const COMPACT_QUERY = '(max-width: 768px)';

export const SettingsGroup = {
  Workspace: 'workspace',
  Organization: 'organization',
  Server: 'server',
  You: 'you',
} as const;

export type SettingsGroup = (typeof SettingsGroup)[keyof typeof SettingsGroup];

export const SettingsSection = {
  General: 'general',
  Ai: 'ai',
  AgentPolicy: 'agent-policy',
  Folders: 'folders',
  Sources: 'sources',
  Members: 'members',
  Roles: 'roles',
  Devices: 'devices',
  Audit: 'audit',
  AuditExport: 'audit-export',
  Spend: 'spend',
  GitHub: 'github',
  Mcp: 'mcp',
  Features: 'features',
  Connect: 'connect',
  License: 'license',
  Sso: 'sso',
  Sessions: 'sessions',
} as const;

export type SettingsSection = (typeof SettingsSection)[keyof typeof SettingsSection];

export interface SettingsAccess {
  organization: boolean;
  organizationRole: OrgRole | undefined;
  organizationPermissions?: readonly string[] | undefined;
  workspace: boolean;
  workspaceRole: WorkspaceRole | undefined;
  workspacePermissions?: readonly string[] | undefined;
  installsModels: boolean;
  readsSources: boolean;
  administersServer: boolean;
}

export interface SectionDefinition {
  group: SettingsGroup;
  section: SettingsSection;
  label: string;
  visible: (access: SettingsAccess) => boolean;
}

export const GROUP_LABELS: Record<SettingsGroup, string> = {
  workspace: 'Workspace',
  organization: 'Organization',
  server: 'This server',
  you: 'You',
};

const SOURCES_LABEL = 'Sources';

const AGENT_POLICY_LABEL = 'Agent policy';

const AUDIT_EXPORT_LABEL = 'Audit export';

const LOCATION_SEPARATOR = ' → ';

export const SOURCES_LOCATION = [
  SETTINGS_TITLE,
  GROUP_LABELS[SettingsGroup.Workspace],
  SOURCES_LABEL,
].join(LOCATION_SEPARATOR);

export const SOURCES_PATH = settingsPath(SettingsGroup.Workspace, SettingsSection.Sources);

const inWorkspace = (access: SettingsAccess): boolean => access.workspace;

const inOrganization = (access: SettingsAccess): boolean => access.organization;

type OrganizationAccess = Pick<
  SettingsAccess,
  'organization' | 'organizationRole' | 'organizationPermissions'
>;

const grantedInOrganization =
  (name: OrganizationPermission) =>
  (access: OrganizationAccess): boolean =>
    access.organization &&
    grantsOrganization(access.organizationPermissions, access.organizationRole, name);

const installsModels = (access: SettingsAccess): boolean => access.installsModels;

const readsWorkspaceSources = (access: SettingsAccess): boolean =>
  inWorkspace(access) && access.readsSources;

const administersServer = (access: SettingsAccess): boolean => access.administersServer;

const always = (): boolean => true;

export const SECTIONS: readonly SectionDefinition[] = [
  {
    group: SettingsGroup.Workspace,
    section: SettingsSection.General,
    label: 'General',
    visible: inWorkspace,
  },
  {
    group: SettingsGroup.Workspace,
    section: SettingsSection.Ai,
    label: 'AI',
    visible: inWorkspace,
  },
  {
    group: SettingsGroup.Workspace,
    section: SettingsSection.AgentPolicy,
    label: AGENT_POLICY_LABEL,
    visible: inWorkspace,
  },
  {
    group: SettingsGroup.Workspace,
    section: SettingsSection.Folders,
    label: 'Folders',
    visible: inWorkspace,
  },
  {
    group: SettingsGroup.Workspace,
    section: SettingsSection.Sources,
    label: SOURCES_LABEL,
    visible: readsWorkspaceSources,
  },
  {
    group: SettingsGroup.Workspace,
    section: SettingsSection.Members,
    label: 'Members',
    visible: inWorkspace,
  },
  {
    group: SettingsGroup.Workspace,
    section: SettingsSection.Mcp,
    label: 'MCP servers',
    visible: inWorkspace,
  },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.Ai,
    label: 'AI defaults',
    visible: grantedInOrganization(Permission.AiSettingsRead),
  },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.AgentPolicy,
    label: AGENT_POLICY_LABEL,
    visible: inOrganization,
  },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.Members,
    label: 'Members',
    visible: inOrganization,
  },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.Roles,
    label: 'Roles',
    visible: grantedInOrganization(Permission.RolesUpdate),
  },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.Devices,
    label: 'Devices',
    visible: grantedInOrganization(Permission.DevicesRead),
  },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.Audit,
    label: 'Audit log',
    visible: grantedInOrganization(Permission.AuditLogsRead),
  },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.AuditExport,
    label: AUDIT_EXPORT_LABEL,
    visible: grantedInOrganization(Permission.AuditUpdate),
  },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.Spend,
    label: 'Spend',
    visible: administersOrganization,
  },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.GitHub,
    label: 'GitHub',
    visible: inOrganization,
  },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.Mcp,
    label: 'MCP servers',
    visible: inOrganization,
  },
  {
    group: SettingsGroup.Server,
    section: SettingsSection.Features,
    label: 'Features',
    visible: installsModels,
  },
  {
    group: SettingsGroup.Server,
    section: SettingsSection.Connect,
    label: 'Connect a device',
    visible: always,
  },
  {
    group: SettingsGroup.Server,
    section: SettingsSection.License,
    label: 'License',
    visible: administersServer,
  },
  {
    group: SettingsGroup.Server,
    section: SettingsSection.Sso,
    label: 'Single sign-on',
    visible: administersServer,
  },
  {
    group: SettingsGroup.Server,
    section: SettingsSection.AuditExport,
    label: AUDIT_EXPORT_LABEL,
    visible: administersServer,
  },
  {
    group: SettingsGroup.You,
    section: SettingsSection.Sessions,
    label: 'Sessions',
    visible: always,
  },
];

export function settingsPath(group: SettingsGroup, section: SettingsSection): string {
  return `${SETTINGS_ROOT}/${group}/${section}`;
}

export function visibleSections(access: SettingsAccess): SectionDefinition[] {
  return SECTIONS.filter((definition) => definition.visible(access));
}

export function findSection(group?: string, section?: string): SectionDefinition | undefined {
  return SECTIONS.find(
    (definition) => definition.group === group && definition.section === section
  );
}
