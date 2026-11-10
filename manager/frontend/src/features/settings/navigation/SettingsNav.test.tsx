import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { chooseSelect } from '../../../test/select';
import type { Organization, Workspace } from '../../../types';
import type SettingsNavType from './SettingsNav';
import { type SettingsAccess, visibleSections } from './sections';

const mockSetCurrentWorkspace = mock<(workspace: Workspace) => void>();

interface NavContext {
  currentOrganization: Organization | null;
  currentWorkspace: Workspace | null;
  workspaces: Workspace[];
  setCurrentWorkspace: (workspace: Workspace) => void;
}

const workspaceContext: NavContext = {
  currentOrganization: null,
  currentWorkspace: null,
  workspaces: [],
  setCurrentWorkspace: mockSetCurrentWorkspace,
};

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => workspaceContext,
}));

let SettingsNav: typeof SettingsNavType;

beforeAll(async () => {
  SettingsNav = (await import('./SettingsNav')).default;
});

afterAll(() => {
  mock.restore();
});

const organization: Organization = {
  id: 'organization-1',
  name: 'Acme',
  slug: 'acme',
  description: null,
  is_active: true,
  role: 'owner',
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

const workspace = (id: string, name: string): Workspace => ({
  id,
  organization_id: organization.id,
  name,
  slug: name.toLowerCase(),
  description: null,
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
});

const alpha = workspace('workspace-alpha', 'Alpha');
const beta = workspace('workspace-beta', 'Beta');

const ownerAccess: SettingsAccess = {
  organization: true,
  organizationRole: 'owner',
  workspace: true,
  workspaceRole: 'admin',
  installsModels: true,
};

function renderNav(access: SettingsAccess, entry = '/settings/workspace/general') {
  return render(
    <MemoryRouter initialEntries={[entry]}>
      <SettingsNav sections={visibleSections(access)} />
    </MemoryRouter>
  );
}

function linkLabels(group: string): string[] {
  return within(screen.getByRole('group', { name: group }))
    .getAllByRole('link')
    .map((link) => link.textContent ?? '');
}

describe('SettingsNav', () => {
  beforeEach(() => {
    mockSetCurrentWorkspace.mockReset();
    workspaceContext.currentOrganization = organization;
    workspaceContext.currentWorkspace = alpha;
    workspaceContext.workspaces = [alpha];
  });

  it('renders one labelled group per group in order inside the settings navigation', () => {
    renderNav(ownerAccess);

    const nav = screen.getByRole('navigation', { name: 'Settings' });
    const headings = within(nav)
      .getAllByRole('heading', { level: 2 })
      .map((heading) => heading.textContent);
    expect(headings).toEqual(['Workspace', 'Organization', 'This server', 'You']);
    expect(within(nav).getAllByRole('group')).toHaveLength(4);
  });

  it('links every visible section to its settings path', () => {
    renderNav(ownerAccess);

    const links = screen
      .getAllByRole('link')
      .map((link) => [link.textContent, link.getAttribute('href')]);
    expect(links).toEqual([
      ['General', '/settings/workspace/general'],
      ['AI', '/settings/workspace/ai'],
      ['Folders', '/settings/workspace/folders'],
      ['Members', '/settings/workspace/members'],
      ['MCP servers', '/settings/workspace/mcp'],
      ['AI defaults', '/settings/organization/ai'],
      ['Members', '/settings/organization/members'],
      ['Devices', '/settings/organization/devices'],
      ['Billing', '/settings/organization/billing'],
      ['Audit log', '/settings/organization/audit'],
      ['GitHub', '/settings/organization/github'],
      ['MCP servers', '/settings/organization/mcp'],
      ['Features', '/settings/server/features'],
      ['Connect a device', '/settings/server/connect'],
      ['Sessions', '/settings/you/sessions'],
    ]);
  });

  it('marks the current section as the current page', () => {
    renderNav(ownerAccess, '/settings/organization/billing');

    const current = screen
      .getAllByRole('link')
      .filter((link) => link.getAttribute('aria-current') === 'page');
    expect(current.map((link) => link.getAttribute('href'))).toEqual([
      '/settings/organization/billing',
    ]);
  });

  it('omits a group with no visible sections', () => {
    workspaceContext.currentWorkspace = null;
    workspaceContext.workspaces = [];

    renderNav({
      organization: true,
      organizationRole: 'admin',
      workspace: false,
      workspaceRole: undefined,
      installsModels: true,
    });

    expect(screen.queryByRole('group', { name: 'Workspace' })).not.toBeInTheDocument();
    expect(linkLabels('Organization')).toEqual([
      'AI defaults',
      'Members',
      'Devices',
      'Billing',
      'Audit log',
      'GitHub',
      'MCP servers',
    ]);
  });

  it('names the current workspace and organization under their headings', () => {
    renderNav(ownerAccess);

    expect(
      within(screen.getByRole('group', { name: 'Workspace' })).getByText('Alpha')
    ).toBeInTheDocument();
    expect(
      within(screen.getByRole('group', { name: 'Organization' })).getByText('Acme')
    ).toBeInTheDocument();
  });

  it('names the workspace once, in the switch, when there is more than one', () => {
    workspaceContext.workspaces = [alpha, beta];

    renderNav(ownerAccess);

    const group = within(screen.getByRole('group', { name: 'Workspace' }));
    expect(group.queryByText('Alpha', { selector: 'p' })).not.toBeInTheDocument();
    expect(group.getByRole('combobox', { name: 'Switch workspace' })).toHaveTextContent('Alpha');
    expect(
      within(screen.getByRole('group', { name: 'Organization' })).getByText('Acme', {
        selector: 'p',
      })
    ).toBeInTheDocument();
  });

  it('offers no workspace switch with a single workspace', () => {
    renderNav(ownerAccess);

    expect(screen.queryByRole('combobox', { name: 'Switch workspace' })).not.toBeInTheDocument();
  });

  it('switches to the chosen workspace when there is more than one', () => {
    workspaceContext.workspaces = [alpha, beta];

    renderNav(ownerAccess);

    const trigger = within(screen.getByRole('group', { name: 'Workspace' })).getByRole('combobox', {
      name: 'Switch workspace',
    });
    expect(trigger).toHaveTextContent('Alpha');

    chooseSelect('Switch workspace', 'Beta');

    expect(mockSetCurrentWorkspace.mock.calls).toEqual([[beta]]);
  });
});
