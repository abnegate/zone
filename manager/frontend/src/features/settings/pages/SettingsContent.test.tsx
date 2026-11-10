import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen } from '@testing-library/react';
import type { OrgRole, WorkspaceRole } from '../../../types';
import { type SettingsAccess, SettingsGroup, SettingsSection } from '../navigation/sections';

const organizationId = 'org-1';
const workspaceId = 'ws-1';
const organizationDefaultsPath = '/settings/organization/ai';

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
  BillingSection: marker('billing-section'),
  DevicesSection: marker('devices-section'),
  GitHubSection: marker('github-section'),
  OrganizationAiSection: marker('organization-ai-section'),
  OrganizationMembersSection: marker('organization-members-section'),
}));

mock.module('../mcp', () => ({
  McpServersSection: marker('mcp-servers-section'),
}));

mock.module('../server/components', () => ({
  ConnectDevicesSection: marker('connect-devices-section'),
  FeaturesSection: marker('features-section'),
}));

mock.module('../../auth/components', () => ({
  SessionsSection: marker('sessions-section'),
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
    const cases: { role: WorkspaceRole; theme: boolean; ai: boolean; folders: boolean }[] = [
      { role: 'viewer', theme: false, ai: false, folders: false },
      { role: 'member', theme: true, ai: false, folders: true },
      { role: 'admin', theme: true, ai: true, folders: true },
    ];

    for (const { role, theme, ai, folders } of cases) {
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
    }

    it('treats an unresolved workspace role as editable and leaves enforcement to the server', () => {
      renderSection(SettingsGroup.Workspace, SettingsSection.Ai, access(undefined));
      expect(propsOf('workspace-ai-section')).toEqual({ editable: true });
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

  describe('organization invitations', () => {
    const cases: { role: OrgRole; invitations: boolean }[] = [
      { role: 'member', invitations: false },
      { role: 'admin', invitations: true },
      { role: 'owner', invitations: true },
    ];

    for (const { role, invitations } of cases) {
      it(`${invitations ? 'offers' : 'hides'} invitations for an organization ${role}`, () => {
        renderSection(SettingsGroup.Organization, SettingsSection.Members, access('member', role));
        expect(propsOf('organization-members-section')).toEqual({ invitations });
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

    it('lets an organization member connect a device', () => {
      renderSection(SettingsGroup.Server, SettingsSection.Connect, access('viewer', 'member'));
      expect(propsOf('connect-devices-section')).toEqual({});
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
        section: SettingsSection.Members,
        testId: 'workspace-members-section',
        props: { workspaceId, orgId: organizationId },
      },
      {
        group: SettingsGroup.Organization,
        section: SettingsSection.Ai,
        testId: 'organization-ai-section',
        props: {},
      },
      {
        group: SettingsGroup.Organization,
        section: SettingsSection.Devices,
        testId: 'devices-section',
        props: { orgId: organizationId },
      },
      {
        group: SettingsGroup.Organization,
        section: SettingsSection.Billing,
        testId: 'billing-section',
        props: { orgId: organizationId },
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
          section={SettingsSection.Billing}
          access={access('admin', 'admin')}
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
});
