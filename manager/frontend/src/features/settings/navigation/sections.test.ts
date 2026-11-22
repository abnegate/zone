import { describe, expect, it } from 'bun:test';
import { ORGANIZATION_PERMISSIONS, Permission } from '../../enterprise/roles/schemas';
import {
  COMPACT_QUERY,
  findSection,
  GROUP_LABELS,
  SECTIONS,
  SETTINGS_ROOT,
  SETTINGS_TITLE,
  type SettingsAccess,
  SettingsGroup,
  SettingsSection,
  SOURCES_LOCATION,
  SOURCES_PATH,
  settingsPath,
  visibleSections,
} from './sections';

const ALL_SECTIONS = [
  { group: SettingsGroup.Workspace, section: SettingsSection.General, label: 'General' },
  { group: SettingsGroup.Workspace, section: SettingsSection.Ai, label: 'AI' },
  {
    group: SettingsGroup.Workspace,
    section: SettingsSection.AgentPolicy,
    label: 'Agent policy',
  },
  { group: SettingsGroup.Workspace, section: SettingsSection.Folders, label: 'Folders' },
  { group: SettingsGroup.Workspace, section: SettingsSection.Sources, label: 'Sources' },
  { group: SettingsGroup.Workspace, section: SettingsSection.Members, label: 'Members' },
  { group: SettingsGroup.Workspace, section: SettingsSection.Mcp, label: 'MCP servers' },
  { group: SettingsGroup.Organization, section: SettingsSection.Ai, label: 'AI defaults' },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.AgentPolicy,
    label: 'Agent policy',
  },
  { group: SettingsGroup.Organization, section: SettingsSection.Members, label: 'Members' },
  { group: SettingsGroup.Organization, section: SettingsSection.Roles, label: 'Roles' },
  { group: SettingsGroup.Organization, section: SettingsSection.Devices, label: 'Devices' },
  { group: SettingsGroup.Organization, section: SettingsSection.Audit, label: 'Audit log' },
  {
    group: SettingsGroup.Organization,
    section: SettingsSection.AuditExport,
    label: 'Audit export',
  },
  { group: SettingsGroup.Organization, section: SettingsSection.Spend, label: 'Spend' },
  { group: SettingsGroup.Organization, section: SettingsSection.GitHub, label: 'GitHub' },
  { group: SettingsGroup.Organization, section: SettingsSection.Mcp, label: 'MCP servers' },
  { group: SettingsGroup.Server, section: SettingsSection.Features, label: 'Features' },
  { group: SettingsGroup.Server, section: SettingsSection.Connect, label: 'Connect a device' },
  { group: SettingsGroup.Server, section: SettingsSection.License, label: 'License' },
  { group: SettingsGroup.Server, section: SettingsSection.Sso, label: 'Single sign-on' },
  { group: SettingsGroup.Server, section: SettingsSection.AuditExport, label: 'Audit export' },
  { group: SettingsGroup.You, section: SettingsSection.Sessions, label: 'Sessions' },
] as const;

const paths = (access: SettingsAccess): string[] =>
  visibleSections(access).map(({ group, section }) => settingsPath(group, section));

const pathsOf = (...groups: SettingsGroup[]): string[] =>
  ALL_SECTIONS.filter(({ group }) => groups.includes(group)).map(({ group, section }) =>
    settingsPath(group, section)
  );

const WORKSPACE_AGENT_POLICY = settingsPath(SettingsGroup.Workspace, SettingsSection.AgentPolicy);
const ORGANIZATION_AGENT_POLICY = settingsPath(
  SettingsGroup.Organization,
  SettingsSection.AgentPolicy
);
const ORGANIZATION_MEMBERS = settingsPath(SettingsGroup.Organization, SettingsSection.Members);
const ORGANIZATION_GITHUB = settingsPath(SettingsGroup.Organization, SettingsSection.GitHub);
const ORGANIZATION_MCP = settingsPath(SettingsGroup.Organization, SettingsSection.Mcp);
const FEATURES = settingsPath(SettingsGroup.Server, SettingsSection.Features);
const CONNECT = settingsPath(SettingsGroup.Server, SettingsSection.Connect);
const LICENSE = settingsPath(SettingsGroup.Server, SettingsSection.License);
const SSO = settingsPath(SettingsGroup.Server, SettingsSection.Sso);
const SERVER_AUDIT_EXPORT = settingsPath(SettingsGroup.Server, SettingsSection.AuditExport);
const ORGANIZATION_AUDIT_EXPORT = settingsPath(
  SettingsGroup.Organization,
  SettingsSection.AuditExport
);
const ORGANIZATION_ROLES = settingsPath(SettingsGroup.Organization, SettingsSection.Roles);
const ORGANIZATION_SPEND = settingsPath(SettingsGroup.Organization, SettingsSection.Spend);
const SESSIONS = settingsPath(SettingsGroup.You, SettingsSection.Sessions);
const SOURCES = settingsPath(SettingsGroup.Workspace, SettingsSection.Sources);

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
    expect(findSection('workspace', 'mcp')).toMatchObject({
      group: SettingsGroup.Workspace,
      section: SettingsSection.Mcp,
      label: 'MCP servers',
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
    expect(findSection('organization', 'mcp')).toMatchObject({
      group: SettingsGroup.Organization,
      section: SettingsSection.Mcp,
      label: 'MCP servers',
    });
    expect(findSection('you', 'sessions')).toMatchObject({
      group: SettingsGroup.You,
      section: SettingsSection.Sessions,
    });
  });

  it('finds nothing for a section that belongs to another group', () => {
    expect(findSection('workspace', 'audit')).toBeUndefined();
    expect(findSection('server', 'members')).toBeUndefined();
  });

  it('finds nothing for an unknown group or section', () => {
    expect(findSection('nowhere', 'ai')).toBeUndefined();
    expect(findSection('workspace', 'nothing')).toBeUndefined();
    expect(findSection('organization', 'billing')).toBeUndefined();
  });

  it('finds nothing when the group or section is missing', () => {
    expect(findSection()).toBeUndefined();
    expect(findSection('workspace')).toBeUndefined();
    expect(findSection(undefined, 'ai')).toBeUndefined();
  });
});

describe('visibleSections', () => {
  it('shows an organization member in a workspace as a viewer only the organization agent policy, members list, GitHub and MCP servers', () => {
    expect(
      paths({
        organization: true,
        organizationRole: 'member',
        workspace: true,
        workspaceRole: 'viewer',
        installsModels: false,
        readsSources: true,
        administersServer: false,
      })
    ).toEqual([
      ...pathsOf(SettingsGroup.Workspace),
      ORGANIZATION_AGENT_POLICY,
      ORGANIZATION_MEMBERS,
      ORGANIZATION_GITHUB,
      ORGANIZATION_MCP,
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
        readsSources: true,
        administersServer: true,
      })
    ).toEqual(pathsOf(SettingsGroup.Organization, SettingsGroup.Server, SettingsGroup.You));
  });

  it('shows all twenty-three sections to a server admin who owns the organization and administers the workspace', () => {
    const visible = paths({
      organization: true,
      organizationRole: 'owner',
      workspace: true,
      workspaceRole: 'admin',
      installsModels: true,
      readsSources: true,
      administersServer: true,
    });

    expect(visible).toHaveLength(23);
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
        readsSources: true,
        administersServer: true,
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
        readsSources: false,
        administersServer: false,
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
        readsSources: true,
        administersServer: false,
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
        readsSources: false,
        administersServer: false,
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
          readsSources: false,
          administersServer: false,
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
          readsSources: true,
          administersServer: false,
        }).filter((path) => path.startsWith(`${SETTINGS_ROOT}/${SettingsGroup.Workspace}/`)),
        workspaceRole
      ).toEqual(pathsOf(SettingsGroup.Workspace));
    }
  });

  it('shows Sources in a workspace to whoever can read sources', () => {
    for (const workspaceRole of ['viewer', 'member'] as const) {
      expect(
        paths({
          organization: true,
          organizationRole: 'member',
          workspace: true,
          workspaceRole,
          installsModels: false,
          readsSources: true,
          administersServer: false,
        }),
        workspaceRole
      ).toContain(SOURCES);
    }
  });

  it('hides Sources in a workspace from whoever cannot read sources', () => {
    const visible = paths({
      organization: true,
      organizationRole: 'owner',
      workspace: true,
      workspaceRole: 'admin',
      installsModels: true,
      readsSources: false,
      administersServer: false,
    });

    expect(visible).not.toContain(SOURCES);
    expect(
      visible.filter((path) => path.startsWith(`${SETTINGS_ROOT}/${SettingsGroup.Workspace}/`))
    ).toEqual(pathsOf(SettingsGroup.Workspace).filter((path) => path !== SOURCES));
  });

  it('hides Sources outside a workspace even from whoever can read sources', () => {
    expect(
      paths({
        organization: true,
        organizationRole: 'owner',
        workspace: false,
        workspaceRole: undefined,
        installsModels: true,
        readsSources: true,
        administersServer: false,
      })
    ).not.toContain(SOURCES);
  });

  it('shows License under This server only to a server admin', () => {
    const license = (administersServer: boolean) =>
      paths({
        organization: true,
        organizationRole: 'owner',
        workspace: true,
        workspaceRole: 'admin',
        installsModels: true,
        readsSources: true,
        administersServer,
      }).includes(LICENSE);

    expect(license(true)).toBe(true);
    expect(license(false)).toBe(false);
  });

  it('shows Single sign-on under This server only to a server admin', () => {
    const sso = (administersServer: boolean) =>
      paths({
        organization: true,
        organizationRole: 'owner',
        workspace: true,
        workspaceRole: 'admin',
        installsModels: true,
        readsSources: true,
        administersServer,
      }).includes(SSO);

    expect(sso(true)).toBe(true);
    expect(sso(false)).toBe(false);
  });

  it('shows License, Single sign-on and Audit export to a server admin outside any organization, after Connect a device', () => {
    expect(
      paths({
        organization: false,
        organizationRole: undefined,
        workspace: false,
        workspaceRole: undefined,
        installsModels: false,
        readsSources: false,
        administersServer: true,
      })
    ).toEqual([CONNECT, LICENSE, SSO, SERVER_AUDIT_EXPORT, SESSIONS]);
  });

  it('shows the organization Audit export only to organization owners and admins', () => {
    for (const [organizationRole, shown] of [
      ['owner', true],
      ['admin', true],
      ['member', false],
      [undefined, true],
    ] as const) {
      expect(
        paths({
          organization: true,
          organizationRole,
          workspace: true,
          workspaceRole: 'viewer',
          installsModels: false,
          readsSources: false,
          administersServer: false,
        }).includes(ORGANIZATION_AUDIT_EXPORT),
        String(organizationRole)
      ).toBe(shown);
    }
  });

  it('shows Roles only to organization owners and admins', () => {
    for (const [organizationRole, shown] of [
      ['owner', true],
      ['admin', true],
      ['member', false],
      [undefined, true],
    ] as const) {
      expect(
        paths({
          organization: true,
          organizationRole,
          workspace: true,
          workspaceRole: 'viewer',
          installsModels: false,
          readsSources: false,
          administersServer: false,
        }).includes(ORGANIZATION_ROLES),
        String(organizationRole)
      ).toBe(shown);
    }
  });

  it('hides Roles outside an organization', () => {
    expect(
      paths({
        organization: false,
        organizationRole: undefined,
        workspace: false,
        workspaceRole: undefined,
        installsModels: true,
        readsSources: true,
        administersServer: true,
      })
    ).not.toContain(ORGANIZATION_ROLES);
  });

  it('finds Roles in the organization group only', () => {
    expect(findSection(SettingsGroup.Organization, 'roles')).toMatchObject({
      group: SettingsGroup.Organization,
      section: SettingsSection.Roles,
      label: 'Roles',
    });
    expect(findSection(SettingsGroup.Workspace, 'roles')).toBeUndefined();
    expect(ORGANIZATION_ROLES).toBe('/settings/organization/roles');
  });

  it('shows Spend only to organization owners and admins, right after the organization Audit export', () => {
    for (const [organizationRole, shown] of [
      ['owner', true],
      ['admin', true],
      ['member', false],
      [undefined, true],
    ] as const) {
      const visible = paths({
        organization: true,
        organizationRole,
        workspace: true,
        workspaceRole: 'viewer',
        installsModels: false,
        readsSources: false,
        administersServer: false,
      });
      expect(visible.includes(ORGANIZATION_SPEND), String(organizationRole)).toBe(shown);
      if (shown) {
        expect(visible.indexOf(ORGANIZATION_SPEND), String(organizationRole)).toBe(
          visible.indexOf(ORGANIZATION_AUDIT_EXPORT) + 1
        );
      }
    }
  });

  it('hides Spend outside an organization and finds it only in the organization group', () => {
    expect(
      paths({
        organization: false,
        organizationRole: undefined,
        workspace: false,
        workspaceRole: undefined,
        installsModels: true,
        readsSources: true,
        administersServer: true,
      })
    ).not.toContain(ORGANIZATION_SPEND);
    expect(findSection(SettingsGroup.Organization, 'spend')).toMatchObject({
      group: SettingsGroup.Organization,
      section: SettingsSection.Spend,
      label: 'Spend',
    });
    expect(findSection(SettingsGroup.Workspace, 'spend')).toBeUndefined();
    expect(findSection(SettingsGroup.Server, 'spend')).toBeUndefined();
    expect(ORGANIZATION_SPEND).toBe('/settings/organization/spend');
  });

  it('shows the server Audit export only to a server admin', () => {
    const shown = (administersServer: boolean) =>
      paths({
        organization: true,
        organizationRole: 'owner',
        workspace: true,
        workspaceRole: 'admin',
        installsModels: true,
        readsSources: true,
        administersServer,
      }).includes(SERVER_AUDIT_EXPORT);

    expect(shown(true)).toBe(true);
    expect(shown(false)).toBe(false);
  });

  it('finds Audit export in the organization and server groups at a kebab-case path', () => {
    for (const group of [SettingsGroup.Organization, SettingsGroup.Server]) {
      expect(findSection(group, 'audit-export'), group).toMatchObject({
        group,
        section: SettingsSection.AuditExport,
        label: 'Audit export',
      });
    }
    expect(findSection(SettingsGroup.Workspace, 'audit-export')).toBeUndefined();
    expect(ORGANIZATION_AUDIT_EXPORT).toBe('/settings/organization/audit-export');
    expect(SERVER_AUDIT_EXPORT).toBe('/settings/server/audit-export');
  });

  it('finds the License section in the server group', () => {
    expect(findSection(SettingsGroup.Server, SettingsSection.License)).toMatchObject({
      group: SettingsGroup.Server,
      section: SettingsSection.License,
      label: 'License',
    });
    expect(settingsPath(SettingsGroup.Server, SettingsSection.License)).toBe(
      '/settings/server/license'
    );
  });

  it('finds the Single sign-on section in the server group', () => {
    expect(findSection(SettingsGroup.Server, SettingsSection.Sso)).toMatchObject({
      group: SettingsGroup.Server,
      section: SettingsSection.Sso,
      label: 'Single sign-on',
    });
    expect(settingsPath(SettingsGroup.Server, SettingsSection.Sso)).toBe('/settings/server/sso');
  });

  it('shows Agent policy to every organization member and in every workspace', () => {
    for (const organizationRole of ['owner', 'admin', 'member', undefined] as const) {
      for (const workspaceRole of ['viewer', 'member', 'admin', 'owner', undefined] as const) {
        const visible = paths({
          organization: true,
          organizationRole,
          workspace: true,
          workspaceRole,
          installsModels: false,
          readsSources: false,
          administersServer: false,
        });
        expect(visible, `${organizationRole}/${workspaceRole}`).toContain(
          ORGANIZATION_AGENT_POLICY
        );
        expect(visible, `${organizationRole}/${workspaceRole}`).toContain(WORKSPACE_AGENT_POLICY);
      }
    }
  });

  it('hides Agent policy outside an organization and outside a workspace', () => {
    const visible = paths({
      organization: false,
      organizationRole: undefined,
      workspace: false,
      workspaceRole: undefined,
      installsModels: true,
      readsSources: true,
      administersServer: true,
    });

    expect(visible).not.toContain(ORGANIZATION_AGENT_POLICY);
    expect(visible).not.toContain(WORKSPACE_AGENT_POLICY);
  });

  it('finds Agent policy in the organization and workspace groups at a kebab-case path', () => {
    for (const group of [SettingsGroup.Organization, SettingsGroup.Workspace]) {
      expect(findSection(group, 'agent-policy'), group).toMatchObject({
        group,
        section: SettingsSection.AgentPolicy,
        label: 'Agent policy',
      });
    }
    expect(ORGANIZATION_AGENT_POLICY).toBe('/settings/organization/agent-policy');
    expect(WORKSPACE_AGENT_POLICY).toBe('/settings/workspace/agent-policy');
  });

  it('finds the Sources section in the workspace group', () => {
    expect(findSection(SettingsGroup.Workspace, SettingsSection.Sources)).toMatchObject({
      group: SettingsGroup.Workspace,
      section: SettingsSection.Sources,
      label: 'Sources',
    });
  });

  it('names the Sources location from the settings title, group and section labels', () => {
    const section = findSection(SettingsGroup.Workspace, SettingsSection.Sources);
    expect(SOURCES_LOCATION).toBe(
      `${SETTINGS_TITLE} → ${GROUP_LABELS[SettingsGroup.Workspace]} → ${section?.label}`
    );
    expect(SOURCES_LOCATION).toBe('Settings → Workspace → Sources');
  });

  it('puts the canonical Sources path under workspace settings', () => {
    expect(SOURCES_PATH).toBe(settingsPath(SettingsGroup.Workspace, SettingsSection.Sources));
    expect(SOURCES_PATH).toBe('/settings/workspace/sources');
  });
});

describe('permission-gated organization sections', () => {
  const ORGANIZATION_AI = settingsPath(SettingsGroup.Organization, SettingsSection.Ai);
  const ORGANIZATION_DEVICES = settingsPath(SettingsGroup.Organization, SettingsSection.Devices);
  const ORGANIZATION_AUDIT = settingsPath(SettingsGroup.Organization, SettingsSection.Audit);

  const holding = (organizationPermissions: readonly string[]): SettingsAccess => ({
    organization: true,
    organizationRole: 'member',
    organizationPermissions,
    workspace: false,
    workspaceRole: undefined,
    installsModels: false,
    readsSources: false,
    administersServer: false,
  });

  const organizationPaths = (access: SettingsAccess): string[] =>
    paths(access).filter((path) => path.startsWith(`${SETTINGS_ROOT}/organization/`));

  it('shows a custom role each section its permissions read, whatever its rank', () => {
    expect(
      organizationPaths(
        holding([Permission.OrganizationRead, Permission.AuditLogsRead, Permission.DevicesRead])
      )
    ).toEqual([
      ORGANIZATION_AGENT_POLICY,
      ORGANIZATION_MEMBERS,
      ORGANIZATION_DEVICES,
      ORGANIZATION_AUDIT,
      ORGANIZATION_GITHUB,
      ORGANIZATION_MCP,
    ]);
  });

  it('gates AI defaults, Roles, Devices, Audit log, Audit export and Spend on their own permissions', () => {
    const gates: [string, string][] = [
      [ORGANIZATION_AI, Permission.AiSettingsRead],
      [ORGANIZATION_ROLES, Permission.RolesUpdate],
      [ORGANIZATION_DEVICES, Permission.DevicesRead],
      [ORGANIZATION_AUDIT, Permission.AuditLogsRead],
      [ORGANIZATION_AUDIT_EXPORT, Permission.AuditUpdate],
      [ORGANIZATION_SPEND, Permission.SpendRead],
    ];
    for (const [path, name] of gates) {
      expect(organizationPaths(holding([Permission.OrganizationRead, name])), name).toContain(path);
      expect(organizationPaths(holding([Permission.OrganizationRead])), name).not.toContain(path);
    }
  });

  it('trusts the permissions over the rank when the server sends both', () => {
    expect(
      organizationPaths({ ...holding([Permission.OrganizationRead]), organizationRole: 'owner' })
    ).toEqual([
      ORGANIZATION_AGENT_POLICY,
      ORGANIZATION_MEMBERS,
      ORGANIZATION_GITHUB,
      ORGANIZATION_MCP,
    ]);
  });

  it('shows every organization section to the built-in admin permission set', () => {
    const admin = ORGANIZATION_PERMISSIONS.filter(
      (name) => name !== Permission.OrganizationDelete && name !== Permission.AdministratorsUpdate
    );
    expect(organizationPaths(holding(admin))).toEqual(pathsOf(SettingsGroup.Organization));
  });
});
