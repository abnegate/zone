import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { focusManager, QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ComponentType } from 'react';
import { StrictMode } from 'react';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { ApiError } from '../../../../api/ApiError';
import { githubQueryKeys } from '../../../github/hooks/githubQueryKeys';
import type { GitHubApp, GitHubConnection, GitHubInstallation } from '../../../github/types';

const githubApi = {
  getConnection: mock(),
  registerApp: mock(),
  createInstallation: mock(),
  syncInstallations: mock(),
  removeInstallation: mock(),
  deleteApp: mock(),
  listRepositories: mock(),
  listBranches: mock(),
};
const submitManifest = mock();
const redirectTo = mock();

mock.module('../../../../api/github', () => ({ githubApi }));
mock.module('../../../github/utils/submitManifest', () => ({ submitManifest }));
mock.module('../../../github/utils/redirectTo', () => ({ redirectTo }));

let GitHubSection: ComponentType<{ organizationId: string; canManage: boolean }>;

beforeAll(async () => {
  ({ GitHubSection } = await import('./index'));
});

afterAll(() => {
  mock.restore();
});

const ORGANIZATION_ID = 'org-1';
const SETTINGS_ENTRY = '/settings/organization/github';
const REMOVED_NOTICE = 'acme was removed from Zone. Delete the app on GitHub to finish.';

function app(id: string, login: string, ownerType: GitHubApp['owner_type']): GitHubApp {
  return {
    id,
    app_id: 100,
    slug: `zone-${login}`,
    name: `Zone (${login})`,
    owner_login: login,
    owner_type: ownerType,
    html_url: `https://github.com/apps/zone-${login}`,
    settings_url: `https://github.com/settings/apps/zone-${login}`,
    created_at: '2026-10-01T00:00:00Z',
  };
}

function installation(
  id: string,
  appId: string,
  createdAt: string,
  overrides: Partial<GitHubInstallation> = {}
): GitHubInstallation {
  return {
    id,
    github_app_id: appId,
    installation_id: 5000,
    account_login: 'globex',
    account_type: 'Organization',
    repository_selection: 'all',
    html_url: `https://github.com/organizations/globex/settings/installations/${id}`,
    suspended_at: null,
    revoked_at: null,
    created_at: createdAt,
    ...overrides,
  };
}

const acme = app('app-acme', 'acme', 'Organization');
const jake = app('app-jake', 'jake', 'User');

const registration = {
  action: 'https://github.com/organizations/acme/settings/apps/new?state=signed-state',
  manifest: '{"name":"Zone"}',
  state: 'signed-state',
};

let connection: GitHubConnection;

function LocationProbe() {
  const location = useLocation();
  return <p data-testid="location">{`${location.pathname}${location.search}`}</p>;
}

function renderSection({
  entry = SETTINGS_ENTRY,
  canManage = true,
  cached,
}: {
  entry?: string;
  canManage?: boolean;
  cached?: GitHubConnection;
} = {}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  if (cached !== undefined) {
    queryClient.setQueryData(githubQueryKeys.connection(ORGANIZATION_ID), cached);
  }
  return render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={[entry]}>
          <GitHubSection organizationId={ORGANIZATION_ID} canManage={canManage} />
          <LocationProbe />
        </MemoryRouter>
      </QueryClientProvider>
    </StrictMode>
  );
}

function location(): string {
  return screen.getByTestId('location').textContent ?? '';
}

function findRow(login: string): Promise<HTMLElement> {
  return screen.findByRole('listitem', { name: login });
}

function findByRoleContaining(role: 'alert' | 'status', text: string): Promise<HTMLElement> {
  return waitFor(() => {
    const match = screen.getAllByRole(role).find((element) => element.textContent?.includes(text));
    if (!match) {
      throw new Error(`No ${role} contains "${text}"`);
    }
    return match;
  });
}

function restorePageFromHistory() {
  act(() => {
    window.dispatchEvent(Object.assign(new Event('pageshow'), { persisted: true }));
  });
}

function connectButton(): HTMLElement {
  return screen.getByRole('button', { name: 'Connect GitHub' });
}

function chooseOrganization(login: string) {
  fireEvent.click(screen.getByRole('radio', { name: 'An organization' }));
  fireEvent.change(screen.getByRole('textbox', { name: 'Organization login' }), {
    target: { value: login },
  });
}

beforeEach(() => {
  for (const method of Object.values(githubApi)) {
    method.mockReset();
  }
  submitManifest.mockReset();
  redirectTo.mockReset();
  connection = { apps: [], installations: [] };
  githubApi.getConnection.mockImplementation(async () => connection);
  githubApi.registerApp.mockResolvedValue(registration);
  githubApi.syncInstallations.mockResolvedValue(undefined);
  githubApi.removeInstallation.mockResolvedValue(undefined);
  githubApi.deleteApp.mockResolvedValue(undefined);
});

describe('GitHubSection', () => {
  describe('structure and loading', () => {
    it('labels the section with its heading and describes what connecting does', async () => {
      renderSection();

      const section = screen.getByRole('region', { name: 'GitHub' });
      expect(within(section).getByRole('heading', { level: 2, name: 'GitHub' })).toBeTruthy();
      expect(
        within(section).getByText(
          'Connect GitHub accounts so members can pick repositories without access tokens.'
        )
      ).toBeTruthy();
      await screen.findByRole('radio', { name: 'Your account' });
    });

    it('announces the connection while it loads', async () => {
      githubApi.getConnection.mockImplementation(() => new Promise(() => {}));
      renderSection();

      await findByRoleContaining('status', 'Loading GitHub connection…');
      expect(githubApi.getConnection).toHaveBeenCalledWith(ORGANIZATION_ID);
      expect(screen.queryByRole('radio', { name: 'Your account' })).toBeNull();
    });

    it('reports a load failure and retries on request', async () => {
      githubApi.getConnection.mockRejectedValueOnce(
        new Error('Failed to load the GitHub connection: 503')
      );
      connection = { apps: [acme], installations: [] };
      renderSection();

      await findByRoleContaining('alert', 'Failed to load the GitHub connection: 503');

      fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

      expect(await findRow('acme')).toBeTruthy();
      expect(screen.queryByText('Failed to load the GitHub connection: 503')).toBeNull();
    });

    it('shows Retry as busy while retrying a failed reload of loaded accounts', async () => {
      connection = { apps: [acme], installations: [] };
      renderSection();
      await findRow('acme');
      githubApi.getConnection.mockRejectedValueOnce(
        new Error('Failed to load the GitHub connection: 503')
      );

      try {
        act(() => {
          focusManager.setFocused(false);
          focusManager.setFocused(true);
        });
        await findByRoleContaining('alert', 'Failed to load the GitHub connection: 503');
      } finally {
        focusManager.setFocused(undefined);
      }
      const retry = screen.getByRole('button', { name: 'Retry' });
      expect(retry).toBeEnabled();
      githubApi.getConnection.mockImplementation(() => new Promise(() => {}));

      fireEvent.click(retry);

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Retry' })).toBeDisabled();
      });
    });
  });

  describe('when no account is connected', () => {
    it('offers to connect your account by default and hides the organization login', async () => {
      renderSection();

      const group = await screen.findByRole('group', { name: 'Connect to' });
      expect(within(group).getByRole('radio', { name: 'Your account' })).toBeChecked();
      expect(within(group).getByRole('radio', { name: 'An organization' })).not.toBeChecked();
      expect(screen.queryByRole('textbox', { name: 'Organization login' })).toBeNull();
      expect(connectButton()).toBeEnabled();
      expect(screen.queryByRole('button', { name: 'Cancel' })).toBeNull();
      expect(screen.queryByRole('list', { name: 'Connected GitHub accounts' })).toBeNull();
    });

    it('asks for an organization login once an organization is chosen', async () => {
      renderSection();

      fireEvent.click(await screen.findByRole('radio', { name: 'An organization' }));

      expect(screen.getByRole('textbox', { name: 'Organization login' })).toBeTruthy();
      expect(
        screen.getByText('You need permission to create GitHub Apps in this organization.')
      ).toBeTruthy();

      fireEvent.click(screen.getByRole('radio', { name: 'Your account' }));

      expect(screen.queryByRole('textbox', { name: 'Organization login' })).toBeNull();
    });

    it('keeps Connect GitHub disabled while the organization login is blank', async () => {
      renderSection();
      await screen.findByRole('radio', { name: 'An organization' });

      chooseOrganization('');
      expect(connectButton()).toBeDisabled();

      fireEvent.change(screen.getByRole('textbox', { name: 'Organization login' }), {
        target: { value: '   ' },
      });
      expect(connectButton()).toBeDisabled();

      fireEvent.change(screen.getByRole('textbox', { name: 'Organization login' }), {
        target: { value: 'acme' },
      });
      expect(connectButton()).toBeEnabled();
    });

    it('keeps Connect GitHub disabled while the registration is pending', async () => {
      githubApi.registerApp.mockImplementation(() => new Promise(() => {}));
      renderSection();
      await screen.findByRole('radio', { name: 'Your account' });

      fireEvent.click(connectButton());

      await waitFor(() => {
        expect(connectButton()).toBeDisabled();
      });
      expect(githubApi.registerApp).toHaveBeenCalledTimes(1);
      expect(submitManifest).not.toHaveBeenCalled();
    });
  });

  describe('connecting GitHub', () => {
    it('registers an app for your account and posts its manifest to GitHub', async () => {
      renderSection();
      await screen.findByRole('radio', { name: 'Your account' });

      fireEvent.click(connectButton());

      await waitFor(() => {
        expect(submitManifest).toHaveBeenCalledTimes(1);
      });
      expect(githubApi.registerApp).toHaveBeenCalledWith(ORGANIZATION_ID, null);
      expect(submitManifest.mock.calls[0][0]).toEqual(registration);
    });

    it('registers an app for the trimmed organization login', async () => {
      renderSection();
      await screen.findByRole('radio', { name: 'An organization' });

      chooseOrganization('  acme  ');
      fireEvent.click(connectButton());

      await waitFor(() => {
        expect(submitManifest).toHaveBeenCalledTimes(1);
      });
      expect(githubApi.registerApp).toHaveBeenCalledWith(ORGANIZATION_ID, 'acme');
      expect(submitManifest.mock.calls[0][0]).toEqual(registration);
    });

    it('says when Zone is already connected to the GitHub account', async () => {
      githubApi.registerApp.mockRejectedValue(
        new ApiError('Failed to connect GitHub: already connected', 409)
      );
      renderSection();
      await screen.findByRole('radio', { name: 'An organization' });

      chooseOrganization('acme');
      fireEvent.click(connectButton());

      await findByRoleContaining('alert', 'Zone is already connected to this GitHub account.');
      expect(submitManifest).not.toHaveBeenCalled();
      await waitFor(() => {
        expect(connectButton()).toBeEnabled();
      });
    });

    it('says when the organization login is not valid', async () => {
      githubApi.registerApp.mockRejectedValue(
        new ApiError('Failed to connect GitHub: invalid owner', 400)
      );
      renderSection();
      await screen.findByRole('radio', { name: 'An organization' });

      chooseOrganization('not a login');
      fireEvent.click(connectButton());

      await findByRoleContaining('alert', "That isn't a valid GitHub organization login.");
      expect(submitManifest).not.toHaveBeenCalled();
    });

    it('shows any other failure as it was reported', async () => {
      githubApi.registerApp.mockRejectedValue(new ApiError('Failed to connect GitHub: 502', 502));
      renderSection();
      await screen.findByRole('radio', { name: 'Your account' });

      fireEvent.click(connectButton());

      await findByRoleContaining('alert', 'Failed to connect GitHub: 502');
      expect(submitManifest).not.toHaveBeenCalled();
    });

    it('enables Connect GitHub again when the page comes back from GitHub through history', async () => {
      renderSection();
      await screen.findByRole('radio', { name: 'Your account' });

      fireEvent.click(connectButton());
      await waitFor(() => {
        expect(submitManifest).toHaveBeenCalledTimes(1);
      });
      expect(connectButton()).toBeDisabled();

      restorePageFromHistory();

      await waitFor(() => {
        expect(connectButton()).toBeEnabled();
      });
      expect(screen.getByRole('radio', { name: 'Your account' })).toBeEnabled();
    });
  });

  describe('connected accounts', () => {
    it('lists one row per app with its login and account kind', async () => {
      connection = {
        apps: [acme, jake],
        installations: [
          installation('inst-acme', acme.id, '2026-10-02T00:00:00Z'),
          installation('inst-jake', jake.id, '2026-10-02T00:00:00Z'),
        ],
      };
      renderSection();

      const list = await screen.findByRole('list', { name: 'Connected GitHub accounts' });
      expect(within(list).getAllByRole('listitem')).toHaveLength(2);

      const acmeRow = within(list).getByRole('listitem', { name: 'acme' });
      expect(within(acmeRow).getAllByText('acme').length).toBeGreaterThan(0);
      expect(within(acmeRow).getByText('Organization')).toBeTruthy();
      expect(within(acmeRow).queryByText('Personal account')).toBeNull();
      expect(within(acmeRow).getByRole('button', { name: 'Remove' })).toBeTruthy();

      const jakeRow = within(list).getByRole('listitem', { name: 'jake' });
      expect(within(jakeRow).getAllByText('jake').length).toBeGreaterThan(0);
      expect(within(jakeRow).getByText('Personal account')).toBeTruthy();

      expect(screen.queryByRole('radio', { name: 'Your account' })).toBeNull();
      expect(screen.getByRole('button', { name: 'Connect another account' })).toBeTruthy();
    });

    it("links Configure on GitHub to the latest installation's page in a new tab", async () => {
      const older = installation('inst-older', acme.id, '2026-10-02T00:00:00Z');
      const latest = installation('inst-latest', acme.id, '2026-10-05T00:00:00Z');
      connection = { apps: [acme], installations: [older, latest] };
      renderSection();

      const row = await findRow('acme');
      const link = within(row).getByRole('link', { name: 'Configure on GitHub' });
      expect(link).toHaveAttribute('href', latest.html_url);
      expect(link).toHaveAttribute('target', '_blank');
      expect(link).toHaveAttribute('rel', 'noopener noreferrer');
      expect(within(row).queryByRole('button', { name: 'Finish installing' })).toBeNull();
      expect(within(row).queryByText('Installation not finished')).toBeNull();
    });

    it('shows no badge when an older revoked installation was replaced by an active one', async () => {
      const revoked = installation('inst-revoked', acme.id, '2026-10-02T00:00:00Z', {
        revoked_at: '2026-10-03T00:00:00Z',
      });
      const active = installation('inst-active', acme.id, '2026-10-04T00:00:00Z');
      connection = { apps: [acme], installations: [active, revoked] };
      renderSection();

      const row = await findRow('acme');
      expect(within(row).queryByText('Revoked')).toBeNull();
      expect(within(row).queryByText('Suspended')).toBeNull();
      expect(within(row).getByRole('link', { name: 'Configure on GitHub' })).toHaveAttribute(
        'href',
        active.html_url
      );
      expect(within(row).queryByRole('button', { name: 'Finish installing' })).toBeNull();
    });

    it('marks a revoked latest installation and offers to finish installing again', async () => {
      const active = installation('inst-active', acme.id, '2026-10-02T00:00:00Z');
      const revoked = installation('inst-revoked', acme.id, '2026-10-04T00:00:00Z', {
        revoked_at: '2026-10-05T00:00:00Z',
      });
      connection = { apps: [acme], installations: [active, revoked] };
      renderSection();

      const row = await findRow('acme');
      expect(within(row).getByText('Revoked')).toBeTruthy();
      expect(within(row).getByRole('button', { name: 'Finish installing' })).toBeTruthy();
      expect(within(row).queryByRole('link', { name: 'Configure on GitHub' })).toBeNull();
    });

    it('marks an app whose only installation is revoked and offers to finish installing', async () => {
      connection = {
        apps: [acme],
        installations: [
          installation('inst-revoked', acme.id, '2026-10-02T00:00:00Z', {
            revoked_at: '2026-10-03T00:00:00Z',
            suspended_at: '2026-10-02T12:00:00Z',
          }),
        ],
      };
      renderSection();

      const row = await findRow('acme');
      expect(within(row).getByText('Revoked')).toBeTruthy();
      expect(within(row).queryByText('Suspended')).toBeNull();
      expect(within(row).queryByText('Installation not finished')).toBeNull();
      expect(within(row).getByRole('button', { name: 'Finish installing' })).toBeTruthy();
      expect(within(row).queryByRole('link', { name: 'Configure on GitHub' })).toBeNull();
    });

    it('marks a suspended latest installation and still links to configure it', async () => {
      const suspended = installation('inst-suspended', acme.id, '2026-10-02T00:00:00Z', {
        suspended_at: '2026-10-03T00:00:00Z',
      });
      connection = { apps: [acme], installations: [suspended] };
      renderSection();

      const row = await findRow('acme');
      expect(within(row).getByText('Suspended')).toBeTruthy();
      expect(within(row).queryByText('Revoked')).toBeNull();
      expect(within(row).getByRole('link', { name: 'Configure on GitHub' })).toHaveAttribute(
        'href',
        suspended.html_url
      );
      expect(within(row).queryByRole('button', { name: 'Finish installing' })).toBeNull();
    });

    it('says the installation is not finished only for an app without installations', async () => {
      connection = {
        apps: [acme, jake],
        installations: [
          installation('inst-jake', jake.id, '2026-10-02T00:00:00Z', {
            revoked_at: '2026-10-03T00:00:00Z',
          }),
        ],
      };
      renderSection();

      const acmeRow = await findRow('acme');
      expect(within(acmeRow).getByText('Installation not finished')).toBeTruthy();
      expect(within(acmeRow).queryByText('Revoked')).toBeNull();
      expect(within(acmeRow).getByRole('button', { name: 'Finish installing' })).toBeTruthy();

      const jakeRow = screen.getByRole('listitem', { name: 'jake' });
      expect(within(jakeRow).queryByText('Installation not finished')).toBeNull();
      expect(within(jakeRow).getByText('Revoked')).toBeTruthy();
    });

    it('starts an installation and sends the browser to GitHub to finish it', async () => {
      const url = 'https://github.com/apps/zone-acme/installations/new?state=install-state';
      githubApi.createInstallation.mockResolvedValue(url);
      connection = { apps: [acme], installations: [] };
      renderSection();

      const row = await findRow('acme');
      fireEvent.click(within(row).getByRole('button', { name: 'Finish installing' }));

      await waitFor(() => {
        expect(redirectTo).toHaveBeenCalledWith(url);
      });
      expect(githubApi.createInstallation).toHaveBeenCalledWith(ORGANIZATION_ID, acme.id);
      expect(redirectTo).toHaveBeenCalledTimes(1);
    });

    it('enables Finish installing again when the page comes back from GitHub through history', async () => {
      githubApi.createInstallation.mockResolvedValue(
        'https://github.com/apps/zone-acme/installations/new?state=install-state'
      );
      connection = { apps: [acme], installations: [] };
      renderSection();

      const row = await findRow('acme');
      fireEvent.click(within(row).getByRole('button', { name: 'Finish installing' }));
      await waitFor(() => {
        expect(redirectTo).toHaveBeenCalledTimes(1);
      });
      expect(within(row).getByRole('button', { name: 'Finish installing' })).toBeDisabled();

      restorePageFromHistory();

      await waitFor(() => {
        expect(within(row).getByRole('button', { name: 'Finish installing' })).toBeEnabled();
      });
    });

    it('reveals the connect form for another account and hides it on Cancel', async () => {
      connection = {
        apps: [acme],
        installations: [installation('inst-acme', acme.id, '2026-10-02T00:00:00Z')],
      };
      renderSection();
      await findRow('acme');

      fireEvent.click(screen.getByRole('button', { name: 'Connect another account' }));

      expect(screen.getByRole('radio', { name: 'Your account' })).toBeChecked();
      expect(connectButton()).toBeEnabled();

      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      expect(screen.queryByRole('radio', { name: 'Your account' })).toBeNull();
      expect(screen.queryByRole('button', { name: 'Connect GitHub' })).toBeNull();
      expect(screen.getByRole('button', { name: 'Connect another account' })).toBeTruthy();
    });

    it('moves focus into the revealed form and back to its trigger on Cancel', async () => {
      connection = {
        apps: [acme],
        installations: [installation('inst-acme', acme.id, '2026-10-02T00:00:00Z')],
      };
      renderSection();
      await findRow('acme');

      const trigger = screen.getByRole('button', { name: 'Connect another account' });
      trigger.focus();
      fireEvent.click(trigger);

      await waitFor(() => {
        expect(document.activeElement).toBe(screen.getByRole('radio', { name: 'Your account' }));
      });

      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      await waitFor(() => {
        expect(document.activeElement).toBe(
          screen.getByRole('button', { name: 'Connect another account' })
        );
      });
    });

    it('leaves focus alone when the form shows because nothing is connected', async () => {
      renderSection();

      const radio = await screen.findByRole('radio', { name: 'Your account' });
      expect(document.activeElement).not.toBe(radio);
    });

    it('connects another account from the revealed form', async () => {
      connection = {
        apps: [acme],
        installations: [installation('inst-acme', acme.id, '2026-10-02T00:00:00Z')],
      };
      renderSection();
      await findRow('acme');

      fireEvent.click(screen.getByRole('button', { name: 'Connect another account' }));
      chooseOrganization('globex');
      fireEvent.click(connectButton());

      await waitFor(() => {
        expect(submitManifest).toHaveBeenCalledTimes(1);
      });
      expect(githubApi.registerApp).toHaveBeenCalledWith(ORGANIZATION_ID, 'globex');
    });
  });

  describe('removing an account', () => {
    const active = installation('inst-active', acme.id, '2026-10-02T00:00:00Z');
    const revoked = installation('inst-revoked', acme.id, '2026-10-03T00:00:00Z', {
      revoked_at: '2026-10-04T00:00:00Z',
    });
    const suspended = installation('inst-suspended', acme.id, '2026-10-05T00:00:00Z', {
      suspended_at: '2026-10-06T00:00:00Z',
    });
    const otherApp = installation('inst-jake', jake.id, '2026-10-02T00:00:00Z');

    beforeEach(() => {
      connection = { apps: [acme, jake], installations: [active, revoked, suspended, otherApp] };
    });

    async function openRemoveDialog(): Promise<HTMLElement> {
      const row = await findRow('acme');
      fireEvent.click(within(row).getByRole('button', { name: 'Remove' }));
      return screen.findByRole('dialog', { name: 'Remove acme?' });
    }

    it('warns about affected sources and offers to uninstall from GitHub by default', async () => {
      renderSection();

      const dialog = await openRemoveDialog();

      expect(
        within(dialog).getByText(
          'Sources using this account stop working until they are reconnected.'
        )
      ).toBeTruthy();
      expect(
        within(dialog).getByRole('checkbox', { name: 'Also uninstall from GitHub' })
      ).toBeChecked();
      expect(within(dialog).getByRole('button', { name: 'Cancel' })).toBeTruthy();
      expect(within(dialog).getByRole('button', { name: 'Remove account' })).toBeTruthy();
    });

    it('closes without removing anything on Cancel', async () => {
      renderSection();

      const dialog = await openRemoveDialog();
      fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));

      await waitFor(() => {
        expect(screen.queryByRole('dialog')).toBeNull();
      });
      expect(githubApi.removeInstallation).not.toHaveBeenCalled();
      expect(githubApi.deleteApp).not.toHaveBeenCalled();
    });

    it('uninstalls every unrevoked installation of the app before deleting it', async () => {
      const calls: string[] = [];
      githubApi.removeInstallation.mockImplementation(
        async (_organizationId: string, installationId: string, uninstall: boolean) => {
          calls.push(`removeInstallation:${installationId}:${uninstall}`);
        }
      );
      githubApi.deleteApp.mockImplementation(async (_organizationId: string, appId: string) => {
        calls.push(`deleteApp:${appId}`);
      });
      renderSection();

      const dialog = await openRemoveDialog();
      fireEvent.click(within(dialog).getByRole('button', { name: 'Remove account' }));

      await waitFor(() => {
        expect(githubApi.deleteApp).toHaveBeenCalledTimes(1);
      });
      expect(githubApi.deleteApp).toHaveBeenCalledWith(ORGANIZATION_ID, acme.id);
      expect(githubApi.removeInstallation).toHaveBeenCalledTimes(2);
      expect(githubApi.removeInstallation).toHaveBeenCalledWith(ORGANIZATION_ID, active.id, true);
      expect(githubApi.removeInstallation).toHaveBeenCalledWith(
        ORGANIZATION_ID,
        suspended.id,
        true
      );
      expect(calls.slice(0, 2).sort()).toEqual([
        `removeInstallation:${active.id}:true`,
        `removeInstallation:${suspended.id}:true`,
      ]);
      expect(calls[2]).toBe(`deleteApp:${acme.id}`);
    });

    it('only deletes the app when the uninstall box is cleared', async () => {
      renderSection();

      const dialog = await openRemoveDialog();
      const uninstall = within(dialog).getByRole('checkbox', {
        name: 'Also uninstall from GitHub',
      });
      fireEvent.click(uninstall);
      expect(uninstall).not.toBeChecked();
      fireEvent.click(within(dialog).getByRole('button', { name: 'Remove account' }));

      await waitFor(() => {
        expect(githubApi.deleteApp).toHaveBeenCalledWith(ORGANIZATION_ID, acme.id);
      });
      expect(githubApi.removeInstallation).not.toHaveBeenCalled();
    });

    it('tells the admin to delete the app on GitHub once it is removed from Zone', async () => {
      githubApi.deleteApp.mockImplementation(async () => {
        connection = { apps: [jake], installations: [otherApp] };
      });
      renderSection();

      const dialog = await openRemoveDialog();
      fireEvent.click(within(dialog).getByRole('button', { name: 'Remove account' }));

      await findByRoleContaining('status', REMOVED_NOTICE);
      await waitFor(() => {
        expect(screen.queryByRole('dialog')).toBeNull();
      });
      const link = screen.getByRole('link', { name: 'Delete app on GitHub' });
      expect(link).toHaveAttribute('href', acme.settings_url);
      expect(link).toHaveAttribute('target', '_blank');
    });

    async function removeAcme() {
      const dialog = await openRemoveDialog();
      const confirm = within(dialog).getByRole('button', { name: 'Remove account' });
      confirm.focus();
      fireEvent.click(confirm);
      await waitFor(() => {
        expect(screen.queryByRole('dialog')).toBeNull();
      });
      await waitFor(() => {
        expect(screen.queryByRole('listitem', { name: 'acme' })).toBeNull();
      });
      await act(() => new Promise((resolve) => setTimeout(resolve, 0)));
    }

    it('moves focus to Connect another account once the removed row is gone', async () => {
      githubApi.deleteApp.mockImplementation(async () => {
        connection = { apps: [jake], installations: [otherApp] };
      });
      renderSection();

      await removeAcme();

      expect(document.activeElement).toBe(
        screen.getByRole('button', { name: 'Connect another account' })
      );
    });

    it('moves focus to the section heading once the last account is removed', async () => {
      connection = { apps: [acme], installations: [active] };
      githubApi.deleteApp.mockImplementation(async () => {
        connection = { apps: [], installations: [] };
      });
      renderSection();

      await removeAcme();

      expect(document.activeElement).toBe(
        screen.getByRole('heading', { level: 2, name: 'GitHub' })
      );
    });

    it('keeps the dialog open with an alert when the removal fails', async () => {
      githubApi.deleteApp.mockRejectedValue(new Error('Failed to remove the GitHub App: 500'));
      renderSection();

      const dialog = await openRemoveDialog();
      fireEvent.click(within(dialog).getByRole('button', { name: 'Remove account' }));

      expect(await within(dialog).findByRole('alert')).toHaveTextContent(
        'Failed to remove the GitHub App: 500'
      );
      expect(screen.getByRole('dialog', { name: 'Remove acme?' })).toBeTruthy();
      expect(screen.queryByText(REMOVED_NOTICE)).toBeNull();
    });
  });

  describe('read-only access', () => {
    it('shows connected accounts to non-admins without any actions', async () => {
      connection = {
        apps: [acme],
        installations: [installation('inst-acme', acme.id, '2026-10-02T00:00:00Z')],
      };
      renderSection({ canManage: false });

      const row = await findRow('acme');
      const section = screen.getByRole('region', { name: 'GitHub' });
      expect(
        within(section).getByText('Only organization admins can change GitHub settings.')
      ).toBeTruthy();
      expect(within(row).getByText('Organization')).toBeTruthy();
      expect(within(section).queryAllByRole('button')).toHaveLength(0);
      expect(within(section).queryAllByRole('link')).toHaveLength(0);
      expect(within(section).queryByText('No GitHub account is connected.')).toBeNull();
    });

    it('tells non-admins that no account is connected instead of offering the form', async () => {
      renderSection({ canManage: false });

      expect(await screen.findByText('No GitHub account is connected.')).toBeTruthy();
      const section = screen.getByRole('region', { name: 'GitHub' });
      expect(
        within(section).getByText('Only organization admins can change GitHub settings.')
      ).toBeTruthy();
      expect(within(section).queryByRole('radio')).toBeNull();
      expect(within(section).queryAllByRole('button')).toHaveLength(0);
      expect(within(section).queryAllByRole('link')).toHaveLength(0);
    });
  });

  describe('returning from GitHub', () => {
    const outcomes: [string, string][] = [
      ['installed', 'GitHub account connected.'],
      ['requested', 'Installation requested. A GitHub organization owner must approve it.'],
      ['expired', 'The GitHub connection link expired. Start again.'],
      ['failed', 'GitHub could not finish connecting. Try again.'],
    ];

    for (const [outcome, text] of outcomes) {
      it(`says "${text}" for github=${outcome} and drops that parameter`, async () => {
        renderSection({ entry: `${SETTINGS_ENTRY}?github=${outcome}` });

        await findByRoleContaining('status', text);
        await waitFor(() => {
          expect(location()).toBe(SETTINGS_ENTRY);
        });
        expect(githubApi.syncInstallations).not.toHaveBeenCalled();
      });

      it(`refetches a cached connection only once for github=${outcome}`, async () => {
        connection = {
          apps: [acme],
          installations: [installation('inst-acme', acme.id, '2026-10-02T00:00:00Z')],
        };
        renderSection({
          entry: `${SETTINGS_ENTRY}?github=${outcome}`,
          cached: { apps: [], installations: [] },
        });

        await findRow('acme');
        await findByRoleContaining('status', text);
        expect(githubApi.getConnection).toHaveBeenCalledTimes(1);
      });
    }

    it('refreshes GitHub accounts once for an admin returning with github=sync', async () => {
      renderSection({ entry: `${SETTINGS_ENTRY}?github=sync` });

      await findByRoleContaining('status', 'GitHub accounts refreshed.');
      expect(githubApi.syncInstallations).toHaveBeenCalledTimes(1);
      expect(githubApi.syncInstallations).toHaveBeenCalledWith(ORGANIZATION_ID);
      await waitFor(() => {
        expect(location()).toBe(SETTINGS_ENTRY);
      });
    });

    it('reports a failed refresh for github=sync', async () => {
      githubApi.syncInstallations.mockRejectedValue(
        new Error('Failed to refresh GitHub accounts: 502')
      );
      renderSection({ entry: `${SETTINGS_ENTRY}?github=sync` });

      const alert = await findByRoleContaining('alert', 'Failed to refresh GitHub accounts: 502');
      expect(alert).toHaveTextContent(/^Failed to refresh GitHub accounts: 502$/);
      expect(screen.queryByText('GitHub accounts refreshed.')).toBeNull();
    });

    it('does not refresh GitHub accounts for a member returning with github=sync', async () => {
      connection = {
        apps: [acme],
        installations: [installation('inst-acme', acme.id, '2026-10-02T00:00:00Z')],
      };
      renderSection({
        entry: `${SETTINGS_ENTRY}?github=sync`,
        canManage: false,
        cached: { apps: [], installations: [] },
      });

      await findRow('acme');
      await waitFor(() => {
        expect(location()).toBe(SETTINGS_ENTRY);
      });
      expect(githubApi.syncInstallations).not.toHaveBeenCalled();
      expect(githubApi.getConnection).toHaveBeenCalledTimes(1);
      expect(screen.queryByText('GitHub accounts refreshed.')).toBeNull();
    });

    it('keeps the other parameters when it drops the outcome', async () => {
      renderSection({ entry: '/settings/organization/github?github=installed&view=compact' });

      await findByRoleContaining('status', 'GitHub account connected.');
      await waitFor(() => {
        expect(location()).toBe('/settings/organization/github?view=compact');
      });
    });
  });
});
