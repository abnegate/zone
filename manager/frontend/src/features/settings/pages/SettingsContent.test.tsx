import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen } from '@testing-library/react';
import type { OrgRole, WorkspaceRole } from '../../../types';
import {
  AdministersServerProvider,
  useAdministersServer,
} from '../../enterprise/components/AdministersServer';
import { Permission } from '../../enterprise/roles/schemas';
import { type SettingsAccess, SettingsGroup, SettingsSection } from '../navigation/sections';

const organizationId = 'org-1';
const workspaceId = 'ws-1';
const organizationDefaultsPath = '/settings/organization/ai';
const organizationPolicyPath = '/settings/organization/agent-policy';

const workspaceContext: {
  currentOrganization: { id: string } | null;
  currentWorkspace: { id: string } | null;
} = {
  currentOrganization: { id: organizationId },
  currentWorkspace: { id: workspaceId },
};

function marker(name: string) {
  return (props: object) => <div data-testid={name}>{JSON.stringify(props)}</div>;
}

function licensed(name: string) {
  return (props: object) => (
    <div data-testid={name} data-administers-server={String(useAdministersServer())}>
      {JSON.stringify(props)}
    </div>
  );
}

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => workspaceContext,
}));

mock.module('../workspace/components', () => ({
  GeneralSection: marker('general-section'),
  HostFoldersPanel: marker('host-folders-panel'),
  WorkspaceAiSection: marker('workspace-ai-section'),
  WorkspaceMembersSection: marker('workspace-members-section'),
}));

mock.module('../organization/components', () => ({
  AuditLogsSection: marker('audit-logs-section'),
  DevicesSection: marker('devices-section'),
  GitHubSection: marker('github-section'),
  OrganizationAiSection: marker('organization-ai-section'),
  OrganizationMembersSection: marker('organization-members-section'),
}));

mock.module('../mcp', () => ({
  McpServersSection: marker('mcp-servers-section'),
  WorkspaceMcpServersSection: marker('workspace-mcp-servers-section'),
}));

mock.module('../server/components', () => ({
  ConnectDevicesSection: marker('connect-devices-section'),
  FeaturesSection: marker('features-section'),
}));

mock.module('../../enterprise', () => ({
  AdministersServerProvider,
  AuditExportSection: licensed('audit-export-section'),
  LicenseSection: marker('license-section'),
  OrganizationPolicySection: licensed('organization-policy-section'),
  organizationScope: (id: string) => ({ kind: 'organization', organizationId: id }),
  Permission,
  RolesSection: licensed('roles-section'),
  RunnersSection: licensed('runners-section'),
  ScimSection: licensed('scim-section'),
  SERVER_SCOPE: { kind: 'server' },
  SpendPage: marker('spend-page'),
  SsoSection: marker('sso-section'),
  WorkspacePolicySection: licensed('workspace-policy-section'),
}));

mock.module('../../auth/components', () => ({
  SessionsSection: marker('sessions-section'),
}));

mock.module('../../sources/components', () => ({
  SourcesSection: marker('sources-section'),
}));

let SettingsContent: typeof import('./SettingsContent').default;

beforeAll(async () => {
  ({ default: SettingsContent } = await import('./SettingsContent'));
});

afterAll(() => {
  mock.restore();
});

function access(
  workspaceRole: WorkspaceRole | undefined,
  organizationRole: OrgRole | undefined = 'member'
): SettingsAccess {
  return {
    organization: true,
    organizationRole,
    workspace: true,
    workspaceRole,
    installsModels: true,
    readsSources: true,
    administersServer: false,
  };
}

function renderSection(
  group: SettingsGroup,
  section: SettingsSection,
  settingsAccess: SettingsAccess
): void {
  render(<SettingsContent group={group} section={section} access={settingsAccess} />);
}

function propsOf(testId: string): unknown {
  return JSON.parse(screen.getByTestId(testId).textContent ?? '');
}

describe('SettingsContent', () => {
  beforeEach(() => {
    workspaceContext.currentOrganization = { id: organizationId };
    workspaceContext.currentWorkspace = { id: workspaceId };
  });

  describe('workspace editability', () => {
    const cases: {
      role: WorkspaceRole;
      theme: boolean;
      ai: boolean;
      folders: boolean;
      sources: boolean;
    }[] = [
      { role: 'viewer', theme: false, ai: false, folders: false, sources: false },
      { role: 'member', theme: true, ai: false, folders: true, sources: true },
      { role: 'admin', theme: true, ai: true, folders: true, sources: true },
      { role: 'owner', theme: true, ai: true, folders: true, sources: true },
    ];

    for (const { role, theme, ai, folders, sources } of cases) {
      it(`lets a workspace ${role} edit only what the role allows`, () => {
        renderSection(SettingsGroup.Workspace, SettingsSection.General, access(role));
        expect(propsOf('general-section')).toEqual({ editable: theme });
      });

      it(`passes editable ${ai} to workspace AI for a workspace ${role}`, () => {
        renderSection(SettingsGroup.Workspace, SettingsSection.Ai, access(role));
        expect(propsOf('workspace-ai-section')).toEqual({ editable: ai });
      });

      it(`makes folders ${folders ? 'writable' : 'read-only'} for a workspace ${role}`, () => {
        renderSection(SettingsGroup.Workspace, SettingsSection.Folders, access(role));
        expect(propsOf('host-folders-panel')).toEqual({
          workspaceId,
          orgId: organizationId,
          readOnly: !folders,
        });
      });

      it(`makes sources ${sources ? 'writable' : 'read-only'} for a workspace ${role}`, () => {
        renderSection(SettingsGroup.Workspace, SettingsSection.Sources, access(role));
        expect(propsOf('sources-section')).toEqual({ readOnly: !sources });
      });
    }

    it('treats an unresolved workspace role as editable and leaves enforcement to the server', () => {
      renderSection(SettingsGroup.Workspace, SettingsSection.Ai, access(undefined));
      expect(propsOf('workspace-ai-section')).toEqual({ editable: true });
    });
  });

  describe('workspace MCP servers', () => {
    const cases: { role: WorkspaceRole | undefined; editable: boolean }[] = [
      { role: 'viewer', editable: false },
      { role: 'member', editable: false },
      { role: 'admin', editable: true },
      { role: 'owner', editable: true },
      { role: undefined, editable: true },
    ];

    for (const { role, editable } of cases) {
      it(`${editable ? 'lets' : 'stops'} a workspace ${role ?? 'role the server never says'} ${editable ? 'change' : 'from changing'} MCP server overrides`, () => {
        renderSection(SettingsGroup.Workspace, SettingsSection.Mcp, access(role));
        expect(propsOf('workspace-mcp-servers-section')).toEqual({
          organizationId,
          workspaceId,
          editable,
        });
      });
    }

    it('renders no MCP servers without a current workspace', () => {
      workspaceContext.currentWorkspace = null;
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Workspace}
          section={SettingsSection.Mcp}
          access={access('admin', 'admin')}
        />
      );
      expect(container.innerHTML).toBe('');
    });

    it('renders no MCP servers without a current organization', () => {
      workspaceContext.currentOrganization = null;
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Workspace}
          section={SettingsSection.Mcp}
          access={access('admin', 'admin')}
        />
      );
      expect(container.innerHTML).toBe('');
    });
  });

  describe('organization defaults link', () => {
    const cases: { role: OrgRole; path: string | undefined }[] = [
      { role: 'member', path: undefined },
      { role: 'admin', path: organizationDefaultsPath },
      { role: 'owner', path: organizationDefaultsPath },
    ];

    for (const { role, path } of cases) {
      it(`${path ? 'links' : 'does not link'} an organization ${role} to the organization defaults`, () => {
        renderSection(SettingsGroup.Workspace, SettingsSection.Ai, access('admin', role));
        expect(propsOf('workspace-ai-section')).toEqual(
          path === undefined
            ? { editable: true }
            : { editable: true, organizationDefaultsPath: path }
        );
      });
    }
  });

  describe('agent policy', () => {
    const cases: { role: OrgRole; path: string | undefined }[] = [
      { role: 'member', path: undefined },
      { role: 'admin', path: organizationPolicyPath },
      { role: 'owner', path: organizationPolicyPath },
    ];

    for (const { role, path } of cases) {
      it(`${path ? 'links' : 'does not link'} an organization ${role} from the workspace policy to the organization policy`, () => {
        renderSection(SettingsGroup.Workspace, SettingsSection.AgentPolicy, access('viewer', role));
        expect(propsOf('workspace-policy-section')).toEqual(
          path === undefined
            ? { organizationId, workspaceId }
            : { organizationId, workspaceId, organizationPolicyPath: path }
        );
      });
    }

    for (const role of ['member', 'admin', 'owner'] as const) {
      it(`shows the organization policy to an organization ${role} and leaves editing to the server`, () => {
        renderSection(
          SettingsGroup.Organization,
          SettingsSection.AgentPolicy,
          access('viewer', role)
        );
        expect(propsOf('organization-policy-section')).toEqual({ organizationId });
      });
    }

    it('renders no workspace policy without a current workspace', () => {
      workspaceContext.currentWorkspace = null;
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Workspace}
          section={SettingsSection.AgentPolicy}
          access={access('admin', 'admin')}
        />
      );
      expect(container.innerHTML).toBe('');
    });

    it('renders no organization policy without a current organization', () => {
      workspaceContext.currentOrganization = null;
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Organization}
          section={SettingsSection.AgentPolicy}
          access={access('admin', 'admin')}
        />
      );
      expect(container.innerHTML).toBe('');
    });
  });

  describe('organization invitations', () => {
    const cases: { role: OrgRole; invitations: boolean }[] = [
      { role: 'member', invitations: false },
      { role: 'admin', invitations: true },
      { role: 'owner', invitations: true },
    ];

    for (const { role, invitations } of cases) {
      it(`${invitations ? 'offers' : 'hides'} invitations for an organization ${role}`, () => {
        renderSection(SettingsGroup.Organization, SettingsSection.Members, access('member', role));
        expect(propsOf('organization-members-section')).toEqual({
          invitations: { read: invitations, create: invitations, revoke: invitations, role },
        });
      });
    }
  });

  describe('organization GitHub', () => {
    const cases: { role: OrgRole | undefined; canManage: boolean }[] = [
      { role: 'member', canManage: false },
      { role: 'admin', canManage: true },
      { role: 'owner', canManage: true },
      { role: undefined, canManage: true },
    ];

    for (const { role, canManage } of cases) {
      it(`lets an organization ${role ?? 'role the server never says'} ${canManage ? 'manage' : 'only view'} GitHub`, () => {
        renderSection(SettingsGroup.Organization, SettingsSection.GitHub, {
          ...access('member'),
          organizationRole: role,
        });
        expect(propsOf('github-section')).toEqual({ organizationId, canManage });
      });
    }

    it('renders no GitHub section without a current organization', () => {
      workspaceContext.currentOrganization = null;
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Organization}
          section={SettingsSection.GitHub}
          access={access('admin', 'admin')}
        />
      );
      expect(container.innerHTML).toBe('');
    });
  });

  describe('organization MCP servers', () => {
    const cases: { role: OrgRole | undefined; canManage: boolean }[] = [
      { role: 'member', canManage: false },
      { role: 'admin', canManage: true },
      { role: 'owner', canManage: true },
      { role: undefined, canManage: true },
    ];

    for (const { role, canManage } of cases) {
      it(`lets an organization ${role ?? 'role the server never says'} ${canManage ? 'manage' : 'only view'} MCP servers`, () => {
        renderSection(SettingsGroup.Organization, SettingsSection.Mcp, {
          ...access('member'),
          organizationRole: role,
        });
        expect(propsOf('mcp-servers-section')).toEqual({ organizationId, canManage });
      });
    }

    it('renders no MCP servers section without a current organization', () => {
      workspaceContext.currentOrganization = null;
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Organization}
          section={SettingsSection.Mcp}
          access={access('admin', 'admin')}
        />
      );
      expect(container.innerHTML).toBe('');
    });
  });

  describe('organization roles', () => {
    it('renders the roles of the current organization for an organization admin', () => {
      renderSection(SettingsGroup.Organization, SettingsSection.Roles, access('member', 'admin'));
      expect(propsOf('roles-section')).toEqual({ organizationId });
    });

    it.each([true, false])(
      'tells the roles whether the viewer administers the server (%p), so license notices link only where the License page is reachable',
      (administersServer) => {
        renderSection(SettingsGroup.Organization, SettingsSection.Roles, {
          ...access('member', 'owner'),
          administersServer,
        });
        expect(screen.getByTestId('roles-section')).toHaveAttribute(
          'data-administers-server',
          String(administersServer)
        );
      }
    );

    it.each([
      [SettingsGroup.Organization, SettingsSection.AgentPolicy, 'organization-policy-section'],
      [SettingsGroup.Workspace, SettingsSection.AgentPolicy, 'workspace-policy-section'],
      [SettingsGroup.Organization, SettingsSection.AuditExport, 'audit-export-section'],
    ])(
      'keeps %s %s license notices from linking a viewer who does not administer the server',
      (group, section, testId) => {
        renderSection(group, section, access('admin', 'owner'));
        expect(screen.getByTestId(testId)).toHaveAttribute('data-administers-server', 'false');
      }
    );

    it('renders no roles for an organization member', () => {
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Organization}
          section={SettingsSection.Roles}
          access={access('member', 'member')}
        />
      );
      expect(container.innerHTML).toBe('');
    });

    it('renders no roles without a current organization', () => {
      workspaceContext.currentOrganization = null;
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Organization}
          section={SettingsSection.Roles}
          access={access('admin', 'owner')}
        />
      );
      expect(container.innerHTML).toBe('');
    });
  });

  describe('organization SCIM provisioning', () => {
    it('renders the SCIM provisioning of the current organization for an organization owner', () => {
      renderSection(SettingsGroup.Organization, SettingsSection.Scim, access('member', 'owner'));
      expect(propsOf('scim-section')).toEqual({ organizationId });
    });

    it.each([true, false])(
      'tells SCIM provisioning whether the viewer administers the server (%p), so license notices link only where the License page is reachable',
      (administersServer) => {
        renderSection(SettingsGroup.Organization, SettingsSection.Scim, {
          ...access('member', 'owner'),
          administersServer,
        });
        expect(screen.getByTestId('scim-section')).toHaveAttribute(
          'data-administers-server',
          String(administersServer)
        );
      }
    );

    it.each(['admin', 'member'] as const)(
      'renders no SCIM provisioning for an organization %s',
      (organizationRole) => {
        const { container } = render(
          <SettingsContent
            group={SettingsGroup.Organization}
            section={SettingsSection.Scim}
            access={access('admin', organizationRole)}
          />
        );
        expect(container.innerHTML).toBe('');
      }
    );

    it('renders no SCIM provisioning without a current organization', () => {
      workspaceContext.currentOrganization = null;
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Organization}
          section={SettingsSection.Scim}
          access={access('admin', 'owner')}
        />
      );
      expect(container.innerHTML).toBe('');
    });
  });

  describe('server features', () => {
    const cases: { role: OrgRole | undefined; installsModels: boolean; shown: boolean }[] = [
      { role: 'member', installsModels: true, shown: true },
      { role: 'admin', installsModels: true, shown: true },
      { role: 'owner', installsModels: false, shown: false },
      { role: undefined, installsModels: false, shown: false },
    ];

    for (const { role, installsModels, shown } of cases) {
      it(`${shown ? 'shows' : 'hides'} server-wide features for an organization ${role ?? 'role the server never says'} ${installsModels ? 'who can' : 'who cannot'} install models`, () => {
        renderSection(SettingsGroup.Server, SettingsSection.Features, {
          ...access('member', role),
          installsModels,
        });
        expect(screen.queryByTestId('features-section') !== null).toBe(shown);
      });
    }

    it('shows the license to a server admin', () => {
      renderSection(SettingsGroup.Server, SettingsSection.License, {
        ...access('viewer', 'member'),
        administersServer: true,
      });
      expect(propsOf('license-section')).toEqual({});
    });

    it('renders no license for anyone who does not administer the server', () => {
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Server}
          section={SettingsSection.License}
          access={access('admin', 'owner')}
        />
      );
      expect(container.innerHTML).toBe('');
    });

    it('shows single sign-on to a server admin', () => {
      renderSection(SettingsGroup.Server, SettingsSection.Sso, {
        ...access('viewer', 'member'),
        administersServer: true,
      });
      expect(propsOf('sso-section')).toEqual({});
    });

    it('renders no single sign-on for anyone who does not administer the server', () => {
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Server}
          section={SettingsSection.Sso}
          access={access('admin', 'owner')}
        />
      );
      expect(container.innerHTML).toBe('');
    });

    it('shows the runners to a server admin', () => {
      renderSection(SettingsGroup.Server, SettingsSection.Runners, {
        ...access('viewer', 'member'),
        administersServer: true,
      });
      expect(propsOf('runners-section')).toEqual({});
      expect(screen.getByTestId('runners-section').dataset.administersServer).toBe('true');
    });

    it('renders no runners for anyone who does not administer the server', () => {
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Server}
          section={SettingsSection.Runners}
          access={access('admin', 'owner')}
        />
      );
      expect(container.innerHTML).toBe('');
    });

    it('lets an organization member connect a device', () => {
      renderSection(SettingsGroup.Server, SettingsSection.Connect, access('viewer', 'member'));
      expect(propsOf('connect-devices-section')).toEqual({});
    });
  });

  describe('audit export', () => {
    for (const role of ['admin', 'owner'] as const) {
      it(`shows an organization ${role} the organization audit export`, () => {
        renderSection(
          SettingsGroup.Organization,
          SettingsSection.AuditExport,
          access('viewer', role)
        );
        expect(propsOf('audit-export-section')).toEqual({
          scope: { kind: 'organization', organizationId },
        });
      });
    }

    it('renders no organization audit export for an organization member', () => {
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Organization}
          section={SettingsSection.AuditExport}
          access={access('admin', 'member')}
        />
      );
      expect(container.innerHTML).toBe('');
    });

    it('renders no organization audit export without a current organization', () => {
      workspaceContext.currentOrganization = null;
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Organization}
          section={SettingsSection.AuditExport}
          access={access('admin', 'admin')}
        />
      );
      expect(container.innerHTML).toBe('');
    });

    it('shows a server admin the server-wide audit export', () => {
      renderSection(SettingsGroup.Server, SettingsSection.AuditExport, {
        ...access('viewer', 'member'),
        administersServer: true,
      });
      expect(propsOf('audit-export-section')).toEqual({ scope: { kind: 'server' } });
    });

    it('renders no server-wide audit export for anyone who does not administer the server', () => {
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Server}
          section={SettingsSection.AuditExport}
          access={access('admin', 'owner')}
        />
      );
      expect(container.innerHTML).toBe('');
    });
  });

  describe('spend', () => {
    for (const role of ['admin', 'owner'] as const) {
      it(`shows an organization ${role} the spend page of the current organization`, () => {
        renderSection(SettingsGroup.Organization, SettingsSection.Spend, access('viewer', role));
        expect(propsOf('spend-page')).toEqual({ organizationId, editable: true });
      });
    }

    it('renders no spend page for an organization member', () => {
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Organization}
          section={SettingsSection.Spend}
          access={access('admin', 'member')}
        />
      );
      expect(container.innerHTML).toBe('');
    });

    it('renders no spend page without a current organization', () => {
      workspaceContext.currentOrganization = null;
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Organization}
          section={SettingsSection.Spend}
          access={access('admin', 'admin')}
        />
      );
      expect(container.innerHTML).toBe('');
    });
  });

  describe('section routing', () => {
    const cases: {
      group: SettingsGroup;
      section: SettingsSection;
      testId: string;
      props: object;
    }[] = [
      {
        group: SettingsGroup.Workspace,
        section: SettingsSection.Sources,
        testId: 'sources-section',
        props: { readOnly: false },
      },
      {
        group: SettingsGroup.Workspace,
        section: SettingsSection.Members,
        testId: 'workspace-members-section',
        props: { workspaceId, orgId: organizationId },
      },
      {
        group: SettingsGroup.Organization,
        section: SettingsSection.Ai,
        testId: 'organization-ai-section',
        props: { editable: true },
      },
      {
        group: SettingsGroup.Organization,
        section: SettingsSection.Devices,
        testId: 'devices-section',
        props: { orgId: organizationId, editable: true },
      },
      {
        group: SettingsGroup.Organization,
        section: SettingsSection.Audit,
        testId: 'audit-logs-section',
        props: { orgId: organizationId },
      },
      {
        group: SettingsGroup.Server,
        section: SettingsSection.Features,
        testId: 'features-section',
        props: {},
      },
      {
        group: SettingsGroup.Server,
        section: SettingsSection.Connect,
        testId: 'connect-devices-section',
        props: {},
      },
      {
        group: SettingsGroup.You,
        section: SettingsSection.Sessions,
        testId: 'sessions-section',
        props: {},
      },
    ];

    for (const { group, section, testId, props } of cases) {
      it(`renders ${testId} for ${group}/${section}`, () => {
        renderSection(group, section, access('admin', 'admin'));
        expect(propsOf(testId)).toEqual(props);
      });
    }

    it('renders nothing for a section its group does not have', () => {
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.You}
          section={SettingsSection.Audit}
          access={access('admin', 'admin')}
        />
      );
      expect(container.innerHTML).toBe('');
    });

    it('renders no sources without permission to read them', () => {
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Workspace}
          section={SettingsSection.Sources}
          access={{ ...access('admin'), readsSources: false }}
        />
      );
      expect(container.innerHTML).toBe('');
    });

    it('renders no sources outside a workspace', () => {
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Workspace}
          section={SettingsSection.Sources}
          access={{ ...access('admin'), workspace: false }}
        />
      );
      expect(container.innerHTML).toBe('');
    });

    it('renders no workspace members without a current workspace', () => {
      workspaceContext.currentWorkspace = null;
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Workspace}
          section={SettingsSection.Members}
          access={access('admin', 'admin')}
        />
      );
      expect(container.innerHTML).toBe('');
    });

    it('renders no organization section without a current organization', () => {
      workspaceContext.currentOrganization = null;
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Organization}
          section={SettingsSection.Devices}
          access={access('admin', 'admin')}
        />
      );
      expect(container.innerHTML).toBe('');
    });
  });

  describe('custom role permissions', () => {
    function holding(
      organizationPermissions: readonly string[] | undefined,
      workspacePermissions: readonly string[] | undefined
    ): SettingsAccess {
      return {
        ...access('viewer', 'member'),
        organizationPermissions,
        workspacePermissions,
      };
    }

    const BASE = [Permission.OrganizationRead];
    const WORKSPACE_BASE = [Permission.WorkspaceRead];

    it('makes workspace AI editable for a viewer whose role grants workspace AI settings', () => {
      renderSection(
        SettingsGroup.Workspace,
        SettingsSection.Ai,
        holding(BASE, [...WORKSPACE_BASE, Permission.WorkspaceAiSettingsUpdate])
      );
      expect(propsOf('workspace-ai-section')).toEqual({ editable: true });
    });

    it('keeps the workspace sections read-only for a role holding only workspace:read, whatever its rank', () => {
      const readOnly = { ...holding(BASE, WORKSPACE_BASE), workspaceRole: 'owner' as const };
      renderSection(SettingsGroup.Workspace, SettingsSection.General, readOnly);
      expect(propsOf('general-section')).toEqual({ editable: false });
    });

    it('opens theme, folders and sources to a role holding workspace:write', () => {
      const writer = holding(BASE, [...WORKSPACE_BASE, Permission.WorkspaceWrite]);
      renderSection(SettingsGroup.Workspace, SettingsSection.General, writer);
      renderSection(SettingsGroup.Workspace, SettingsSection.Folders, writer);
      renderSection(SettingsGroup.Workspace, SettingsSection.Sources, writer);
      expect(propsOf('general-section')).toEqual({ editable: true });
      expect(propsOf('host-folders-panel')).toEqual({
        workspaceId,
        orgId: organizationId,
        readOnly: false,
      });
      expect(propsOf('sources-section')).toEqual({ readOnly: false });
    });

    it('opens workspace MCP overrides only to workspace_mcp_servers:update', () => {
      renderSection(
        SettingsGroup.Workspace,
        SettingsSection.Mcp,
        holding(BASE, [...WORKSPACE_BASE, Permission.WorkspaceWrite])
      );
      expect(propsOf('workspace-mcp-servers-section')).toEqual({
        organizationId,
        workspaceId,
        editable: false,
      });
    });

    it('links workspace AI to the organization defaults with ai_settings:read', () => {
      const linked = holding([...BASE, Permission.AiSettingsRead], WORKSPACE_BASE);
      renderSection(SettingsGroup.Workspace, SettingsSection.Ai, linked);
      expect(propsOf('workspace-ai-section')).toEqual({
        editable: false,
        organizationDefaultsPath,
      });
    });

    it('links the workspace policy to the organization policy only with policies:update', () => {
      renderSection(
        SettingsGroup.Workspace,
        SettingsSection.AgentPolicy,
        holding([...BASE, Permission.PoliciesUpdate], WORKSPACE_BASE)
      );
      expect(propsOf('workspace-policy-section')).toEqual({
        organizationId,
        workspaceId,
        organizationPolicyPath,
      });
    });

    it('shows the organization AI defaults read-only without ai_settings:update', () => {
      renderSection(
        SettingsGroup.Organization,
        SettingsSection.Ai,
        holding([...BASE, Permission.AiSettingsRead], undefined)
      );
      expect(propsOf('organization-ai-section')).toEqual({ editable: false });
    });

    it('shows devices read-only without devices:update', () => {
      renderSection(
        SettingsGroup.Organization,
        SettingsSection.Devices,
        holding([...BASE, Permission.DevicesRead], undefined)
      );
      expect(propsOf('devices-section')).toEqual({ orgId: organizationId, editable: false });
    });

    it('splits the invitation controls by invitations:read, create and delete', () => {
      renderSection(
        SettingsGroup.Organization,
        SettingsSection.Members,
        holding([...BASE, Permission.InvitationsRead, Permission.InvitationsDelete], undefined)
      );
      expect(propsOf('organization-members-section')).toEqual({
        invitations: {
          read: true,
          create: false,
          revoke: true,
          role: 'member',
          permissions: [...BASE, Permission.InvitationsRead, Permission.InvitationsDelete],
        },
      });
    });

    it('lets github:update manage GitHub and mcp_servers:update manage MCP servers', () => {
      const manager = holding([...BASE, Permission.GitHubUpdate], undefined);
      renderSection(SettingsGroup.Organization, SettingsSection.GitHub, manager);
      expect(propsOf('github-section')).toEqual({ organizationId, canManage: true });
      renderSection(SettingsGroup.Organization, SettingsSection.Mcp, manager);
      expect(propsOf('mcp-servers-section')).toEqual({ organizationId, canManage: false });
    });

    it('renders the roles page only with roles:update', () => {
      renderSection(
        SettingsGroup.Organization,
        SettingsSection.Roles,
        holding([...BASE, Permission.RolesUpdate], undefined)
      );
      expect(propsOf('roles-section')).toEqual({ organizationId });
    });

    it('renders the SCIM provisioning page only with scim:update', () => {
      renderSection(
        SettingsGroup.Organization,
        SettingsSection.Scim,
        holding([...BASE, Permission.ScimUpdate], undefined)
      );
      expect(propsOf('scim-section')).toEqual({ organizationId });
    });

    it('renders no SCIM provisioning page to an organization owner rank whose permissions lack scim:update', () => {
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Organization}
          section={SettingsSection.Scim}
          access={{ ...holding(BASE, undefined), organizationRole: 'owner' }}
        />
      );
      expect(container.innerHTML).toBe('');
    });

    it('renders no roles page to an organization admin rank whose permissions lack roles:update', () => {
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Organization}
          section={SettingsSection.Roles}
          access={{ ...holding(BASE, undefined), organizationRole: 'admin' }}
        />
      );
      expect(container.innerHTML).toBe('');
    });

    it('renders the spend page with spend:read, its budgets read-only without spend:update', () => {
      renderSection(
        SettingsGroup.Organization,
        SettingsSection.Spend,
        holding([...BASE, Permission.SpendRead], undefined)
      );
      expect(propsOf('spend-page')).toEqual({ organizationId, editable: false });
    });

    it('lets spend:update change the budgets', () => {
      renderSection(
        SettingsGroup.Organization,
        SettingsSection.Spend,
        holding([...BASE, Permission.SpendRead, Permission.SpendUpdate], undefined)
      );
      expect(propsOf('spend-page')).toEqual({ organizationId, editable: true });
    });

    it('renders no spend page to an organization admin rank whose permissions lack spend:read', () => {
      const { container } = render(
        <SettingsContent
          group={SettingsGroup.Organization}
          section={SettingsSection.Spend}
          access={{ ...holding(BASE, undefined), organizationRole: 'admin' }}
        />
      );
      expect(container.innerHTML).toBe('');
    });
  });
});
