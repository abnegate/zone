import { describe, expect, it } from 'bun:test';
import {
  COMPACT_QUERY,
  findSection,
  GROUP_LABELS,
  SECTIONS,
  SETTINGS_ROOT,
  type SettingsAccess,
  SettingsGroup,
  SettingsSection,
  settingsPath,
  visibleSections,
} from './sections';

const ALL_SECTIONS = [
  { group: SettingsGroup.Workspace, section: SettingsSection.General, label: 'General' },
  { group: SettingsGroup.Workspace, section: SettingsSection.Ai, label: 'AI' },
  { group: SettingsGroup.Workspace, section: SettingsSection.Folders, label: 'Folders' },
  { group: SettingsGroup.Workspace, section: SettingsSection.Members, label: 'Members' },
  { group: SettingsGroup.Organization, section: SettingsSection.Ai, label: 'AI defaults' },
  { group: SettingsGroup.Organization, section: SettingsSection.Members, label: 'Members' },
  { group: SettingsGroup.Organization, section: SettingsSection.Devices, label: 'Devices' },
  { group: SettingsGroup.Organization, section: SettingsSection.Billing, label: 'Billing' },
  { group: SettingsGroup.Organization, section: SettingsSection.Audit, label: 'Audit log' },
  { group: SettingsGroup.Organization, section: SettingsSection.GitHub, label: 'GitHub' },
  { group: SettingsGroup.Server, section: SettingsSection.Features, label: 'Features' },
  { group: SettingsGroup.Server, section: SettingsSection.Connect, label: 'Connect a device' },
  { group: SettingsGroup.You, section: SettingsSection.Sessions, label: 'Sessions' },
] as const;

const paths = (access: SettingsAccess): string[] =>
  visibleSections(access).map(({ group, section }) => settingsPath(group, section));

const pathsOf = (...groups: SettingsGroup[]): string[] =>
  ALL_SECTIONS.filter(({ group }) => groups.includes(group)).map(({ group, section }) =>
    settingsPath(group, section)
  );

const ORGANIZATION_MEMBERS = settingsPath(SettingsGroup.Organization, SettingsSection.Members);
const ORGANIZATION_GITHUB = settingsPath(SettingsGroup.Organization, SettingsSection.GitHub);
const FEATURES = settingsPath(SettingsGroup.Server, SettingsSection.Features);
const CONNECT = settingsPath(SettingsGroup.Server, SettingsSection.Connect);
const SESSIONS = settingsPath(SettingsGroup.You, SettingsSection.Sessions);

describe('settings constants', () => {
  it('roots settings at /settings and switches to the compact layout at the app breakpoint', () => {
    expect(SETTINGS_ROOT).toBe('/settings');
    expect(COMPACT_QUERY).toBe('(max-width: 768px)');
  });

  it('names the groups and sections in the URL', () => {
    expect(SettingsGroup).toEqual({
      Workspace: 'workspace',
      Organization: 'organization',
      Server: 'server',
      You: 'you',
    });
    expect(SettingsSection).toEqual({
      General: 'general',
      Ai: 'ai',
      Folders: 'folders',
      Members: 'members',
      Devices: 'devices',
      Billing: 'billing',
      Audit: 'audit',
      GitHub: 'github',
      Features: 'features',
      Connect: 'connect',
      Sessions: 'sessions',
    });
  });

  it('labels each group for the nav', () => {
    expect(GROUP_LABELS).toEqual({
      workspace: 'Workspace',
      organization: 'Organization',
      server: 'This server',
      you: 'You',
    });
  });

  it('lists every section in nav order with its label', () => {
    expect(SECTIONS.map(({ group, section, label }) => ({ group, section, label }))).toEqual([
      ...ALL_SECTIONS,
    ]);
  });
});

describe('settingsPath', () => {
  it('joins the group and section under the settings root', () => {
    expect(settingsPath(SettingsGroup.Workspace, SettingsSection.Ai)).toBe(
      '/settings/workspace/ai'
    );
    expect(settingsPath(SettingsGroup.Organization, SettingsSection.Audit)).toBe(
      '/settings/organization/audit'
    );
    expect(settingsPath(SettingsGroup.Server, SettingsSection.Connect)).toBe(
      '/settings/server/connect'
    );
    expect(settingsPath(SettingsGroup.You, SettingsSection.Sessions)).toBe(
      '/settings/you/sessions'
    );
  });
});

describe('findSection', () => {
  it('finds a section by its group and section', () => {
    expect(findSection('workspace', 'ai')).toMatchObject({
      group: SettingsGroup.Workspace,
      section: SettingsSection.Ai,
      label: 'AI',
    });
    expect(findSection('organization', 'ai')).toMatchObject({
      group: SettingsGroup.Organization,
      section: SettingsSection.Ai,
      label: 'AI defaults',
    });
    expect(findSection('organization', 'github')).toMatchObject({
      group: SettingsGroup.Organization,
      section: SettingsSection.GitHub,
      label: 'GitHub',
    });
    expect(findSection('you', 'sessions')).toMatchObject({
      group: SettingsGroup.You,
      section: SettingsSection.Sessions,
    });
  });

  it('finds nothing for a section that belongs to another group', () => {
    expect(findSection('workspace', 'billing')).toBeUndefined();
    expect(findSection('server', 'members')).toBeUndefined();
  });

  it('finds nothing for an unknown group or section', () => {
    expect(findSection('nowhere', 'ai')).toBeUndefined();
    expect(findSection('workspace', 'nothing')).toBeUndefined();
  });

  it('finds nothing when the group or section is missing', () => {
    expect(findSection()).toBeUndefined();
    expect(findSection('workspace')).toBeUndefined();
    expect(findSection(undefined, 'ai')).toBeUndefined();
  });
});

describe('visibleSections', () => {
  it('shows an organization member in a workspace as a viewer only the organization members list and GitHub', () => {
    expect(
      paths({
        organization: true,
        organizationRole: 'member',
        workspace: true,
        workspaceRole: 'viewer',
        installsModels: false,
      })
    ).toEqual([
      ...pathsOf(SettingsGroup.Workspace),
      ORGANIZATION_MEMBERS,
      ORGANIZATION_GITHUB,
      CONNECT,
      SESSIONS,
    ]);
  });

  it('hides the workspace group from an organization admin who belongs to no workspace', () => {
    expect(
      paths({
        organization: true,
        organizationRole: 'admin',
        workspace: false,
        workspaceRole: undefined,
        installsModels: true,
      })
    ).toEqual(pathsOf(SettingsGroup.Organization, SettingsGroup.Server, SettingsGroup.You));
  });

  it('shows all thirteen sections to an organization owner who administers the workspace', () => {
    const visible = paths({
      organization: true,
      organizationRole: 'owner',
      workspace: true,
      workspaceRole: 'admin',
      installsModels: true,
    });

    expect(visible).toHaveLength(13);
    expect(visible).toEqual(pathsOf(...Object.values(SettingsGroup)));
  });

  it('shows the admin-only organization sections when the server never says the role', () => {
    expect(
      paths({
        organization: true,
        organizationRole: undefined,
        workspace: true,
        workspaceRole: undefined,
        installsModels: true,
      })
    ).toEqual(pathsOf(...Object.values(SettingsGroup)));
  });

  it('keeps connecting a device and sessions without an organization or workspace', () => {
    expect(
      paths({
        organization: false,
        organizationRole: undefined,
        workspace: false,
        workspaceRole: undefined,
        installsModels: false,
      })
    ).toEqual([CONNECT, SESSIONS]);
  });

  it('shows the server-wide Features section to whoever can install models, as the Models page does', () => {
    const features = (
      organizationRole: SettingsAccess['organizationRole'],
      installsModels: boolean
    ) =>
      paths({
        organization: true,
        organizationRole,
        workspace: true,
        workspaceRole: 'member',
        installsModels,
      }).includes(FEATURES);

    for (const organizationRole of ['owner', 'admin', 'member', undefined] as const) {
      expect(features(organizationRole, true), String(organizationRole)).toBe(true);
      expect(features(organizationRole, false), String(organizationRole)).toBe(false);
    }
  });

  it('shows Features outside an organization when the user can install models', () => {
    expect(
      paths({
        organization: false,
        organizationRole: undefined,
        workspace: false,
        workspaceRole: undefined,
        installsModels: true,
      })
    ).toEqual([FEATURES, CONNECT, SESSIONS]);
  });

  it('lets everyone connect a device', () => {
    for (const organizationRole of ['owner', 'admin', 'member', undefined] as const) {
      expect(
        paths({
          organization: true,
          organizationRole,
          workspace: false,
          workspaceRole: undefined,
          installsModels: false,
        }),
        String(organizationRole)
      ).toContain(CONNECT);
    }
  });

  it('does not let a workspace role hide a workspace section', () => {
    for (const workspaceRole of ['viewer', 'member', 'admin', 'owner'] as const) {
      expect(
        paths({
          organization: true,
          organizationRole: 'member',
          workspace: true,
          workspaceRole,
          installsModels: false,
        }).filter((path) => path.startsWith(`${SETTINGS_ROOT}/${SettingsGroup.Workspace}/`)),
        workspaceRole
      ).toEqual(pathsOf(SettingsGroup.Workspace));
    }
  });
});
