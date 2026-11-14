import type { OrgRole, WorkspaceRole } from '../../../types';
import { allowsOrganization, ORGANIZATION_ADMIN_ROLE } from './access';

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
  Folders: 'folders',
  Sources: 'sources',
  Members: 'members',
  Devices: 'devices',
  Billing: 'billing',
  Audit: 'audit',
  GitHub: 'github',
  Mcp: 'mcp',
  Features: 'features',
  Connect: 'connect',
  Sessions: 'sessions',
} as const;

export type SettingsSection = (typeof SettingsSection)[keyof typeof SettingsSection];

export interface SettingsAccess {
  organization: boolean;
  organizationRole: OrgRole | undefined;
  workspace: boolean;
  workspaceRole: WorkspaceRole | undefined;
  installsModels: boolean;
  readsSources: boolean;
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

const LOCATION_SEPARATOR = ' → ';

export const SOURCES_LOCATION = [
  SETTINGS_TITLE,
  GROUP_LABELS[SettingsGroup.Workspace],
  SOURCES_LABEL,
].join(LOCATION_SEPARATOR);

export const SOURCES_PATH = settingsPath(SettingsGroup.Workspace, SettingsSection.Sources);

const inWorkspace = (access: SettingsAccess): boolean => access.workspace;

const inOrganization = (access: SettingsAccess): boolean => access.organization;

export const administersOrganization = (
  access: Pick<SettingsAccess, 'organization' | 'organizationRole'>
): boolean =>
  access.organization && allowsOrganization(access.organizationRole, ORGANIZATION_ADMIN_ROLE);

const installsModels = (access: SettingsAccess): boolean => access.installsModels;

const readsWorkspaceSources = (access: SettingsAccess): boolean =>
  inWorkspace(access) && access.readsSources;

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
    visible: administersOrganization,
  },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.Members,
    label: 'Members',
    visible: inOrganization,
  },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.Devices,
    label: 'Devices',
    visible: administersOrganization,
  },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.Billing,
    label: 'Billing',
    visible: administersOrganization,
  },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.Audit,
    label: 'Audit log',
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
