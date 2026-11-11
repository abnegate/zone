import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { useEffect } from 'react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { PERMISSIONS } from '../../../shared/types/permissions';
import { chooseSelect } from '../../../test/select';
import type { Organization, OrgRole, Workspace, WorkspaceRole } from '../../../types';
import { COMPACT_QUERY, type SettingsAccess } from '../navigation/sections';
import type { WorkspaceRoleState } from '../navigation/useWorkspaceRole';
import type SettingsPageType from './SettingsPage';

const mockSetCurrentWorkspace = mock<(workspace: Workspace) => void>();

interface PageContext {
  currentOrganization: Organization | null;
  currentWorkspace: Workspace | null;
  workspaces: Workspace[];
  loading: boolean;
  resolvingRole: boolean;
  setCurrentWorkspace: (workspace: Workspace) => void;
}

const workspaceContext: PageContext = {
  currentOrganization: null,
  currentWorkspace: null,
  workspaces: [],
  loading: false,
  resolvingRole: false,
  setCurrentWorkspace: mockSetCurrentWorkspace,
};

const workspaceRole: WorkspaceRoleState = { role: undefined, resolving: false };

let permissions: string[] = [];

const contentMounts: string[] = [];
const contentAccess: SettingsAccess[] = [];

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => workspaceContext,
}));

mock.module('../../auth/context', () => ({
  useAuth: () => ({ hasPermission: (permission: string) => permissions.includes(permission) }),
}));

mock.module('../navigation/useWorkspaceRole', () => ({
  useWorkspaceRole: () => workspaceRole,
}));

mock.module('./SettingsContent', () => ({
  default: function SettingsContent({
    group,
    section,
    access,
  }: {
    group: string;
    section: string;
    access: SettingsAccess;
  }) {
    contentAccess.push(access);
    useEffect(() => {
      contentMounts.push(`${group}/${section}`);
    }, [group, section]);
    return (
      <div data-testid="settings-content">
        {group}/{section}
      </div>
    );
  },
}));

const originalMatchMedia = window.matchMedia;
let compact = false;

let SettingsPage: typeof SettingsPageType;

beforeAll(async () => {
  SettingsPage = (await import('./SettingsPage')).default;
});

afterAll(() => {
  window.matchMedia = originalMatchMedia;
  mock.restore();
});

const organization = (role: OrgRole | undefined): Organization => ({
  id: 'organization-1',
  name: 'Acme',
  slug: 'acme',
  description: null,
  is_active: true,
  role,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
});

const workspace = (id: string, name: string): Workspace => ({
  id,
  organization_id: 'organization-1',
  name,
  slug: name.toLowerCase(),
  description: null,
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
});

const alpha = workspace('workspace-alpha', 'Alpha');
const beta = workspace('workspace-beta', 'Beta');

function signInAs(
  organizationRole: OrgRole | undefined,
  role: WorkspaceRole | null,
  granted: string[] = [PERMISSIONS.MODELS.READ]
) {
  permissions = granted;
  workspaceContext.currentOrganization = organization(organizationRole);
  workspaceContext.currentWorkspace = role === null ? null : alpha;
  workspaceContext.workspaces = role === null ? [] : [alpha];
  workspaceRole.role = role ?? undefined;
  workspaceRole.resolving = false;
}

function LocationProbe() {
  const { pathname, search, hash } = useLocation();
  return (
    <output data-testid="location">
      {pathname}
      {search}
      {hash}
    </output>
  );
}

function tree(entry: string) {
  return (
    <MemoryRouter initialEntries={[entry]}>
      <Routes>
        <Route
          path="/settings/*"
          element={
            <>
              <SettingsPage />
              <LocationProbe />
            </>
          }
        />
        <Route path="*" element={<LocationProbe />} />
      </Routes>
    </MemoryRouter>
  );
}

function renderAt(entry: string) {
  const result = render(tree(entry));
  return { ...result, rerenderAt: () => result.rerender(tree(entry)) };
}

const location = (): string => screen.getByTestId('location').textContent ?? '';

const pageTitle = (): string => screen.getByRole('heading', { level: 1 }).textContent ?? '';

function linkLabels(group: string): string[] {
  return within(screen.getByRole('group', { name: group }))
    .getAllByRole('link')
    .map((link) => link.textContent ?? '');
}

describe('SettingsPage', () => {
  beforeEach(() => {
    compact = false;
    window.matchMedia = ((query: string) => ({
      matches: query === COMPACT_QUERY ? compact : false,
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    })) as unknown as typeof window.matchMedia;
    mockSetCurrentWorkspace.mockReset();
    contentMounts.length = 0;
    contentAccess.length = 0;
    workspaceContext.loading = false;
    workspaceContext.resolvingRole = false;
    signInAs('member', 'viewer');
  });

  describe('visibility', () => {
    it('shows an organization member and workspace viewer the workspace and personal sections', () => {
      signInAs('member', 'viewer');

      renderAt('/settings/workspace/general');

      expect(linkLabels('Workspace')).toEqual(['General', 'AI', 'Folders', 'Members']);
      expect(linkLabels('Organization')).toEqual(['Members', 'GitHub']);
      expect(linkLabels('This server')).toEqual(['Features', 'Connect a device']);
      expect(linkLabels('You')).toEqual(['Sessions']);
    });

    it('shows an organization admin with no workspace every organization section and no workspace group', () => {
      signInAs('admin', null);

      renderAt('/settings/organization/ai');

      expect(screen.queryByRole('group', { name: 'Workspace' })).not.toBeInTheDocument();
      expect(linkLabels('Organization')).toEqual([
        'AI defaults',
        'Members',
        'Devices',
        'Billing',
        'Audit log',
        'GitHub',
      ]);
      expect(linkLabels('This server')).toEqual(['Features', 'Connect a device']);
      expect(linkLabels('You')).toEqual(['Sessions']);
    });

    it('sends a user who cannot install models deep-linking to Features to the first visible section', () => {
      signInAs('owner', 'admin', []);

      renderAt('/settings/server/features');

      expect(location()).toBe('/settings/workspace/general');
      expect(screen.getByTestId('settings-content').textContent).toBe('workspace/general');
      expect(linkLabels('This server')).toEqual(['Connect a device']);
    });

    it('shows Features to an organization member who can install models, like the Models page', () => {
      signInAs('member', 'member');

      renderAt('/settings/server/features');

      expect(location()).toBe('/settings/server/features');
      expect(linkLabels('This server')).toEqual(['Features', 'Connect a device']);
    });

    it('shows an organization owner and workspace admin all thirteen sections', () => {
      signInAs('owner', 'admin');

      renderAt('/settings/workspace/general');

      expect(linkLabels('Workspace')).toEqual(['General', 'AI', 'Folders', 'Members']);
      expect(linkLabels('Organization')).toEqual([
        'AI defaults',
        'Members',
        'Devices',
        'Billing',
        'Audit log',
        'GitHub',
      ]);
      expect(linkLabels('This server')).toEqual(['Features', 'Connect a device']);
      expect(linkLabels('You')).toEqual(['Sessions']);
    });

    it('lists Sources between Folders and Members for a user who can read sources', () => {
      signInAs('member', 'viewer', [PERMISSIONS.MODELS.READ, PERMISSIONS.SOURCES.READ]);

      renderAt('/settings/workspace/sources');

      expect(linkLabels('Workspace')).toEqual(['General', 'AI', 'Folders', 'Sources', 'Members']);
      expect(location()).toBe('/settings/workspace/sources');
      expect(screen.getByTestId('settings-content').textContent).toBe('workspace/sources');
    });

    it('sends a user who cannot read sources deep-linking to Sources to the first visible section', () => {
      signInAs('owner', 'admin');

      renderAt('/settings/workspace/sources?source=abc#details');

      expect(location()).toBe('/settings/workspace/general?source=abc#details');
      expect(screen.getByTestId('settings-content').textContent).toBe('workspace/general');
      expect(linkLabels('Workspace')).toEqual(['General', 'AI', 'Folders', 'Members']);
    });

    it('keeps a user who can read sources deep-linking to a source exactly where they asked', () => {
      signInAs('owner', 'admin', [PERMISSIONS.MODELS.READ, PERMISSIONS.SOURCES.READ]);

      renderAt('/settings/workspace/sources?source=abc#details');

      expect(location()).toBe('/settings/workspace/sources?source=abc#details');
      expect(screen.getByTestId('settings-content').textContent).toBe('workspace/sources');
    });

    it('hides Sources outside a workspace even from a user who can read sources', () => {
      signInAs('admin', null, [PERMISSIONS.MODELS.READ, PERMISSIONS.SOURCES.READ]);

      renderAt('/settings/workspace/sources');

      expect(location()).toBe('/settings/organization/ai');
      expect(screen.queryByRole('link', { name: 'Sources' })).not.toBeInTheDocument();
    });
  });

  describe('desktop', () => {
    it('renders the navigation beside the labelled section content', () => {
      renderAt('/settings/workspace/ai');

      expect(screen.getByRole('navigation', { name: 'Settings' })).toBeInTheDocument();
      const region = screen.getByRole('region', { name: 'AI' });
      expect(within(region).getByTestId('settings-content').textContent).toBe('workspace/ai');
      expect(pageTitle()).toBe('Settings');
      expect(screen.queryByRole('link', { name: 'Back to settings' })).not.toBeInTheDocument();
    });

    it('passes the resolved access to the content', () => {
      signInAs('owner', 'admin');

      renderAt('/settings/workspace/ai');

      expect(contentAccess.at(-1)).toEqual({
        organization: true,
        organizationRole: 'owner',
        workspace: true,
        workspaceRole: 'admin',
        installsModels: true,
        readsSources: false,
      });
    });

    it('passes whether the user can read sources to the content', () => {
      signInAs('owner', 'admin', [PERMISSIONS.MODELS.READ, PERMISSIONS.SOURCES.READ]);

      renderAt('/settings/workspace/sources');

      expect(contentAccess.at(-1)?.readsSources).toBe(true);
    });

    it('sends a hidden deep link to the first visible section', () => {
      renderAt('/settings/organization/billing');

      expect(location()).toBe('/settings/workspace/general');
      expect(screen.getByTestId('settings-content').textContent).toBe('workspace/general');
    });

    it('keeps the query and hash when sending a hidden deep link on', () => {
      renderAt('/settings/organization/billing?x=1#y');

      expect(location()).toBe('/settings/workspace/general?x=1#y');
    });

    it('sends an unknown section to the first visible section', () => {
      renderAt('/settings/organization/nonsense');

      expect(location()).toBe('/settings/workspace/general');
    });

    it('sends an unknown group to the first visible section', () => {
      renderAt('/settings/nonsense/general');

      expect(location()).toBe('/settings/workspace/general');
    });

    it('treats a path deeper than a section as unknown', () => {
      renderAt('/settings/workspace/ai/extra');

      expect(location()).toBe('/settings/workspace/general');
    });

    it('sends the bare path to the first visible section', () => {
      renderAt('/settings');

      expect(location()).toBe('/settings/workspace/general');
    });

    it('keeps the query and hash when sending the bare path on', () => {
      renderAt('/settings?x=1#y');

      expect(location()).toBe('/settings/workspace/general?x=1#y');
    });

    it('sends the bare path to organization AI defaults for an organization admin with no workspace', () => {
      signInAs('admin', null);

      renderAt('/settings');

      expect(location()).toBe('/settings/organization/ai');
    });

    it('remounts the content when the workspace changes', () => {
      workspaceContext.workspaces = [alpha, beta];
      const { rerenderAt } = renderAt('/settings/workspace/general');
      expect(contentMounts).toEqual(['workspace/general']);

      workspaceContext.currentWorkspace = beta;
      rerenderAt();

      expect(contentMounts).toEqual(['workspace/general', 'workspace/general']);
    });

    it('names the current workspace in the switch and switches it from the navigation', () => {
      workspaceContext.workspaces = [alpha, beta];

      renderAt('/settings/workspace/general');

      const group = within(screen.getByRole('group', { name: 'Workspace' }));
      expect(group.queryByText('Alpha', { selector: 'p' })).not.toBeInTheDocument();
      expect(group.getByRole('combobox', { name: 'Switch workspace' })).toHaveTextContent('Alpha');
      chooseSelect('Switch workspace', 'Beta');
      expect(mockSetCurrentWorkspace.mock.calls).toEqual([[beta]]);
    });

    it('offers no workspace switch with a single workspace', () => {
      renderAt('/settings/workspace/general');

      expect(screen.queryByRole('combobox', { name: 'Switch workspace' })).not.toBeInTheDocument();
    });
  });

  describe('legacy workspace tabs', () => {
    beforeEach(() => {
      signInAs('owner', 'admin');
    });

    const cases: readonly [string, string][] = [
      ['theme', '/settings/workspace/general'],
      ['ai', '/settings/workspace/ai'],
      ['features', '/settings/server/features'],
      ['folders', '/settings/workspace/folders'],
      ['members', '/settings/workspace/members'],
      ['devices', '/settings/server/connect'],
    ];

    for (const [tab, path] of cases) {
      it(`sends ?tab=${tab} to ${path}`, () => {
        renderAt(`/settings?tab=${tab}`);

        expect(location()).toBe(path);
        expect(screen.getByTestId('settings-content').textContent).toBe(
          path.replace('/settings/', '')
        );
      });
    }

    it('keeps the other query parameters and the hash', () => {
      renderAt('/settings?tab=features&x=1#y');

      expect(location()).toBe('/settings/server/features?x=1#y');
    });

    it('redirects a legacy tab on a compact screen too', () => {
      compact = true;

      renderAt('/settings?tab=folders');

      expect(location()).toBe('/settings/workspace/folders');
    });

    it('lands on the first visible section when the legacy tab is hidden', () => {
      signInAs('admin', null);

      renderAt('/settings?tab=ai');

      expect(location()).toBe('/settings/organization/ai');
    });

    it('treats an unknown legacy tab as the bare path', () => {
      renderAt('/settings?tab=nonsense');

      expect(location()).toBe('/settings/workspace/general');
    });

    it('sends a user who cannot install models on the old features tab to the first visible section', () => {
      signInAs('owner', 'admin', []);

      renderAt('/settings?tab=features&x=1#y');

      expect(location()).toBe('/settings/workspace/general?x=1#y');
    });
  });

  describe('compact', () => {
    beforeEach(() => {
      compact = true;
    });

    it('shows the section list at the bare path without content', () => {
      renderAt('/settings');

      expect(location()).toBe('/settings');
      expect(screen.getByRole('navigation', { name: 'Settings' })).toBeInTheDocument();
      expect(screen.queryByTestId('settings-content')).not.toBeInTheDocument();
      expect(screen.queryByRole('region')).not.toBeInTheDocument();
      expect(pageTitle()).toBe('Settings');
    });

    it('opens a section from the list with a back link and the section title', () => {
      renderAt('/settings');

      fireEvent.click(
        within(screen.getByRole('group', { name: 'Workspace' })).getByRole('link', { name: 'AI' })
      );

      expect(location()).toBe('/settings/workspace/ai');
      expect(screen.getByTestId('settings-content').textContent).toBe('workspace/ai');
      expect(screen.getByRole('link', { name: 'Back to settings' })).toBeInTheDocument();
      expect(screen.queryByRole('navigation', { name: 'Settings' })).not.toBeInTheDocument();
      expect(pageTitle()).toBe('AI');
    });

    it('returns to the list from the back link', () => {
      renderAt('/settings/server/connect');

      fireEvent.click(screen.getByRole('link', { name: 'Back to settings' }));

      expect(location()).toBe('/settings');
      expect(screen.getByRole('navigation', { name: 'Settings' })).toBeInTheDocument();
      expect(screen.queryByTestId('settings-content')).not.toBeInTheDocument();
      expect(pageTitle()).toBe('Settings');
    });

    it('sends a hidden deep link to the first visible section', () => {
      renderAt('/settings/organization/audit');

      expect(location()).toBe('/settings/workspace/general');
      expect(screen.getByTestId('settings-content').textContent).toBe('workspace/general');
    });
  });

  describe('loading', () => {
    it('shows only the loading state while the workspace context loads', () => {
      workspaceContext.loading = true;

      renderAt('/settings/workspace/general');

      expect(screen.getByText('Loading settings...')).toBeInTheDocument();
      expect(screen.queryByRole('navigation', { name: 'Settings' })).not.toBeInTheDocument();
      expect(screen.queryByTestId('settings-content')).not.toBeInTheDocument();
      expect(screen.queryByRole('region')).not.toBeInTheDocument();
    });

    it('shows only the loading state while the organization role resolves', () => {
      workspaceContext.resolvingRole = true;

      renderAt('/settings');

      expect(location()).toBe('/settings');
      expect(screen.getByText('Loading settings...')).toBeInTheDocument();
      expect(screen.queryByRole('navigation', { name: 'Settings' })).not.toBeInTheDocument();
    });
  });

  describe('while the workspace role resolves', () => {
    beforeEach(() => {
      workspaceRole.role = undefined;
      workspaceRole.resolving = true;
    });

    it('renders the navigation with the content still loading', () => {
      renderAt('/settings/workspace/general');

      expect(screen.getByRole('navigation', { name: 'Settings' })).toBeInTheDocument();
      expect(linkLabels('Workspace')).toEqual(['General', 'AI', 'Folders', 'Members']);
      expect(screen.getByText('Loading settings...')).toBeInTheDocument();
      expect(screen.queryByTestId('settings-content')).not.toBeInTheDocument();
    });

    it('holds the bare path until the role resolves, then redirects', () => {
      const { rerenderAt } = renderAt('/settings');

      expect(location()).toBe('/settings');
      expect(screen.getByRole('navigation', { name: 'Settings' })).toBeInTheDocument();
      expect(screen.getByText('Loading settings...')).toBeInTheDocument();

      workspaceRole.role = 'viewer';
      workspaceRole.resolving = false;
      rerenderAt();

      expect(location()).toBe('/settings/workspace/general');
    });

    it('holds a hidden deep link until the role resolves, then redirects', () => {
      const { rerenderAt } = renderAt('/settings/organization/billing');

      expect(location()).toBe('/settings/organization/billing');
      expect(screen.getByText('Loading settings...')).toBeInTheDocument();
      expect(screen.queryByTestId('settings-content')).not.toBeInTheDocument();

      workspaceRole.role = 'viewer';
      workspaceRole.resolving = false;
      rerenderAt();

      expect(location()).toBe('/settings/workspace/general');
    });

    it('holds a legacy tab until the role resolves', () => {
      renderAt('/settings?tab=features');

      expect(location()).toBe('/settings?tab=features');
    });

    it('renders the compact list at the bare path', () => {
      compact = true;

      renderAt('/settings');

      expect(location()).toBe('/settings');
      expect(screen.getByRole('navigation', { name: 'Settings' })).toBeInTheDocument();
      expect(screen.queryByTestId('settings-content')).not.toBeInTheDocument();
    });
  });
});
