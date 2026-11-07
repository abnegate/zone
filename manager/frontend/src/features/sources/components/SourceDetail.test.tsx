import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { join } from 'node:path';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ComponentProps, ComponentType, ReactNode } from 'react';
import { MemoryRouter } from 'react-router-dom';
import type { GitHubRepositorySearch } from '../../../api/github';
import { read } from '../../../test/css';
import type { OrgRole } from '../../../types';
import type {
  GitHubRepositoriesResponse,
  GitHubRepository,
  GitHubRepositoryInstallation,
} from '../../github/types';
import type { SourceVerifyResponse } from '../schemas';
import type { Source, UpdateSourceRequest } from '../types';

const listRepositories =
  mock<(organizationId: string, search?: GitHubRepositorySearch) => Promise<unknown>>();

mock.module('../../../api/github', () => ({
  githubApi: {
    getConnection: mock(),
    registerApp: mock(),
    createInstallation: mock(),
    syncInstallations: mock(),
    removeInstallation: mock(),
    deleteApp: mock(),
    listRepositories,
    listBranches: mock(),
  },
}));

interface WorkspaceState {
  currentOrganization: { id: string; name: string; role?: OrgRole } | null;
  resolvingRole?: boolean;
}

let workspaceState: WorkspaceState;

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => workspaceState,
  WorkspaceProvider: ({ children }: { children: ReactNode }) => children,
}));

type SourceDetailProps = ComponentProps<typeof import('./SourceDetail').SourceDetail>;

let SourceDetail: ComponentType<SourceDetailProps>;

beforeAll(async () => {
  SourceDetail = (await import('./SourceDetail')).SourceDetail;
});

afterAll(() => {
  mock.restore();
});

const updateSource = mock<(request: UpdateSourceRequest) => Promise<Source>>();
const verifySource = mock<() => Promise<SourceVerifyResponse>>();
const onSaved = mock<() => Promise<void>>();
const onClose = mock<() => void>();

function gitHubSource(overrides: Partial<Source> = {}): Source {
  return {
    id: 'source-1',
    name: 'acme/api',
    source_type: 'github',
    category: 'file',
    config: { owner: 'acme', repo: 'api', branch: 'main' },
    description: null,
    url: 'https://github.com/acme/api',
    is_active: true,
    last_verified_at: null,
    last_error: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

function appSource(overrides: Partial<Source> = {}): Source {
  return gitHubSource({
    github_installation_id: 'installation-1',
    github_installation_status: 'active',
    github_account_login: 'acme',
    has_credentials: false,
    ...overrides,
  });
}

function installation(
  id: string,
  error: GitHubRepositoryInstallation['error'] = null
): GitHubRepositoryInstallation {
  return {
    id,
    account_login: `account-${id}`,
    html_url: `https://github.com/settings/installations/${id}`,
    error,
  };
}

let nextRepositoryId = 1;

function repository(fullName: string, installationId: string): GitHubRepository {
  const [owner, name] = fullName.split('/');
  return {
    installation_id: installationId,
    id: nextRepositoryId++,
    owner,
    name,
    full_name: fullName,
    private: true,
    archived: false,
    default_branch: 'main',
    html_url: `https://github.com/${fullName}`,
  };
}

function repositoriesResponse(
  repositories: GitHubRepository[],
  installations: GitHubRepositoryInstallation[]
): GitHubRepositoriesResponse {
  return { repositories, installations, total: repositories.length, next_page: null };
}

function renderDetail(source: Source) {
  return render(
    <MemoryRouter>
      <SourceDetail
        source={source}
        onClose={onClose}
        onSaved={onSaved}
        updateSource={updateSource}
        verifySource={verifySource}
      />
    </MemoryRouter>
  );
}

function connectionRegion() {
  return screen.getByRole('region', { name: 'GitHub connection' });
}

const UNREACHABLE_MESSAGE = 'No connected GitHub account can reach acme/api.';
const TOKEN_REMOVAL_HINT = 'Your access token will be removed.';

function connectionStatus() {
  return within(connectionRegion()).getByRole('status');
}

async function unreachableStatus() {
  const status = connectionStatus();
  await waitFor(() => expect(status.textContent).toContain(UNREACHABLE_MESSAGE));
  return status;
}

function accessTokenField() {
  return screen.queryByLabelText(/^Access Token/);
}

function expectSearchFor(fullName: string) {
  expect(listRepositories).toHaveBeenCalledTimes(1);
  expect(listRepositories).toHaveBeenCalledWith(
    'org-1',
    expect.objectContaining({ query: fullName, perPage: 100 })
  );
}

beforeEach(() => {
  workspaceState = { currentOrganization: { id: 'org-1', name: 'Acme', role: 'admin' } };
  listRepositories.mockReset();
  updateSource.mockReset();
  updateSource.mockImplementation((request) =>
    Promise.resolve(appSource({ github_installation_id: request.github_installation_id }))
  );
  verifySource.mockReset();
  onSaved.mockReset();
  onSaved.mockResolvedValue(undefined);
  onClose.mockReset();
});

describe('SourceDetail GitHub connection states', () => {
  it('shows an app-backed source with its account login and no action', () => {
    renderDetail(appSource());

    const region = connectionRegion();
    expect(within(region).getByText('Connected through GitHub App (acme)')).toBeTruthy();
    expect(within(region).queryByRole('button')).toBeNull();
  });

  it('omits the login from an app-backed source without one', () => {
    renderDetail(appSource({ github_account_login: null }));

    const region = connectionRegion();
    expect(within(region).getByText('Connected through GitHub App')).toBeTruthy();
    expect(within(region).queryByRole('button')).toBeNull();
  });

  it('treats an installation without a status as active, as older backends send', () => {
    renderDetail(appSource({ github_installation_status: undefined }));

    const region = connectionRegion();
    expect(within(region).getByText('Connected through GitHub App (acme)')).toBeTruthy();
    expect(within(region).queryByRole('button')).toBeNull();
  });

  it('offers Reconnect for a revoked installation', () => {
    renderDetail(appSource({ github_installation_status: 'revoked' }));

    const region = connectionRegion();
    expect(within(region).getByText('GitHub access for acme was revoked.')).toBeTruthy();
    expect(within(region).getByRole('button', { name: 'Reconnect' })).toBeTruthy();
    expect(within(region).queryByRole('button', { name: 'Switch to GitHub App' })).toBeNull();
  });

  it('offers Reconnect for a suspended installation', () => {
    renderDetail(appSource({ github_installation_status: 'suspended' }));

    const region = connectionRegion();
    expect(within(region).getByText('GitHub access for acme is suspended.')).toBeTruthy();
    expect(within(region).getByRole('button', { name: 'Reconnect' })).toBeTruthy();
    expect(within(region).queryByRole('button', { name: 'Switch to GitHub App' })).toBeNull();
  });

  it('omits the login from revoked and suspended installations without one', () => {
    const { unmount } = renderDetail(
      appSource({ github_installation_status: 'revoked', github_account_login: null })
    );
    expect(within(connectionRegion()).getByText('GitHub access was revoked.')).toBeTruthy();
    unmount();

    renderDetail(
      appSource({ github_installation_status: 'suspended', github_account_login: null })
    );
    expect(within(connectionRegion()).getByText('GitHub access is suspended.')).toBeTruthy();
  });

  it('offers Switch to GitHub App for a token-backed source', () => {
    renderDetail(gitHubSource({ github_installation_id: null, has_credentials: true }));

    const region = connectionRegion();
    expect(within(region).getByText('Connected with an access token.')).toBeTruthy();
    expect(within(region).getByRole('button', { name: 'Switch to GitHub App' })).toBeTruthy();
    expect(within(region).queryByRole('button', { name: 'Reconnect' })).toBeNull();
  });

  it('warns that switching to the GitHub App removes the access token', () => {
    renderDetail(gitHubSource({ github_installation_id: null, has_credentials: true }));

    const button = within(connectionRegion()).getByRole('button', {
      name: 'Switch to GitHub App',
    });
    const hintId = button.getAttribute('aria-describedby');
    expect(hintId).toBeTruthy();
    expect(document.getElementById(hintId ?? '')?.textContent).toBe(TOKEN_REMOVAL_HINT);
    expect(within(connectionRegion()).getByText(TOKEN_REMOVAL_HINT)).toBeTruthy();
  });

  it('gives Reconnect no token removal hint', () => {
    const sources = [
      appSource({ github_installation_status: 'revoked' }),
      appSource({ github_installation_status: 'suspended' }),
      gitHubSource({ github_installation_id: null, has_credentials: false }),
    ];
    for (const source of sources) {
      const { unmount } = renderDetail(source);

      const button = within(connectionRegion()).getByRole('button', { name: 'Reconnect' });
      expect(button.getAttribute('aria-describedby')).toBeNull();
      expect(screen.queryByText(TOKEN_REMOVAL_HINT)).toBeNull();
      unmount();
    }
  });

  it('gives an app-backed source no token removal hint', () => {
    renderDetail(appSource());

    expect(screen.queryByText(TOKEN_REMOVAL_HINT)).toBeNull();
  });

  it('treats a source without the credential field as token-backed, as older backends send', () => {
    renderDetail(gitHubSource());

    const region = connectionRegion();
    expect(within(region).getByText('Connected with an access token.')).toBeTruthy();
    expect(within(region).getByRole('button', { name: 'Switch to GitHub App' })).toBeTruthy();
  });

  it('offers Reconnect for a source with neither an installation nor credentials', () => {
    renderDetail(gitHubSource({ github_installation_id: null, has_credentials: false }));

    const region = connectionRegion();
    expect(within(region).getByText('Not connected to GitHub.')).toBeTruthy();
    expect(within(region).getByRole('button', { name: 'Reconnect' })).toBeTruthy();
    expect(within(region).queryByRole('button', { name: 'Switch to GitHub App' })).toBeNull();
  });

  it('renders no GitHub connection region for other source types', () => {
    renderDetail(
      gitHubSource({
        name: 'GitLab project',
        source_type: 'gitlab',
        config: { project_id: '42', host: 'https://gitlab.com' },
        url: 'https://gitlab.com/acme/api',
      })
    );

    expect(screen.queryByRole('region', { name: 'GitHub connection' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Switch to GitHub App' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Reconnect' })).toBeNull();
  });
});

describe('SourceDetail switching to the GitHub App', () => {
  it('links the exact case-insensitive match on a reachable installation', async () => {
    listRepositories.mockResolvedValue(
      repositoriesResponse(
        [
          repository('acme/api-gateway', 'installation-1'),
          repository('acme/api', 'installation-2'),
          repository('ACME/Api', 'installation-1'),
        ],
        [installation('installation-2', 'revoked'), installation('installation-1')]
      )
    );
    renderDetail(gitHubSource({ github_installation_id: null, has_credentials: true }));

    fireEvent.click(screen.getByRole('button', { name: 'Switch to GitHub App' }));

    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expectSearchFor('acme/api');
    expect(updateSource).toHaveBeenCalledTimes(1);
    expect(updateSource).toHaveBeenCalledWith({ github_installation_id: 'installation-1' });
    expect(updateSource.mock.invocationCallOrder[0]).toBeLessThan(
      onSaved.mock.invocationCallOrder[0]
    );
    expect(connectionStatus().textContent).toBe('');
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('skips matches on suspended and unavailable installations', async () => {
    listRepositories.mockResolvedValue(
      repositoriesResponse(
        [
          repository('acme/api', 'installation-2'),
          repository('acme/api', 'installation-3'),
          repository('acme/api', 'installation-1'),
        ],
        [
          installation('installation-2', 'suspended'),
          installation('installation-3', 'unavailable'),
          installation('installation-1'),
        ]
      )
    );
    renderDetail(gitHubSource({ has_credentials: true }));

    fireEvent.click(screen.getByRole('button', { name: 'Switch to GitHub App' }));

    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(updateSource).toHaveBeenCalledWith({ github_installation_id: 'installation-1' });
  });

  for (const status of ['revoked', 'suspended'] as const) {
    it(`reconnects a ${status} source through the same flow`, async () => {
      listRepositories.mockResolvedValue(
        repositoriesResponse(
          [repository('acme/api', 'installation-1'), repository('acme/api', 'installation-2')],
          [installation('installation-1', status), installation('installation-2')]
        )
      );
      renderDetail(appSource({ github_installation_status: status }));

      fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));

      await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
      expectSearchFor('acme/api');
      expect(updateSource).toHaveBeenCalledWith({ github_installation_id: 'installation-2' });
    });
  }

  it('reconnects a disconnected source through the same flow', async () => {
    listRepositories.mockResolvedValue(
      repositoriesResponse(
        [repository('acme/api', 'installation-1')],
        [installation('installation-1')]
      )
    );
    renderDetail(gitHubSource({ github_installation_id: null, has_credentials: false }));

    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));

    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expectSearchFor('acme/api');
    expect(updateSource).toHaveBeenCalledWith({ github_installation_id: 'installation-1' });
  });
});

describe('SourceDetail when no connected account reaches the repository', () => {
  const unreachableResponse = () =>
    repositoriesResponse(
      [repository('acme/api-gateway', 'installation-1'), repository('acme/api', 'installation-2')],
      [installation('installation-1'), installation('installation-2', 'revoked')]
    );

  it('links an admin to the GitHub settings without updating the source', async () => {
    listRepositories.mockResolvedValue(unreachableResponse());
    renderDetail(gitHubSource({ has_credentials: true }));

    fireEvent.click(screen.getByRole('button', { name: 'Switch to GitHub App' }));

    const status = await unreachableStatus();
    const link = within(status).getByRole('link', { name: 'Connect GitHub' });
    expect(link.getAttribute('href')).toBe('/settings/organization/github');
    expect(within(status).queryByText('Ask an organization admin to connect GitHub.')).toBeNull();
    expectSearchFor('acme/api');
    expect(updateSource).not.toHaveBeenCalled();
    expect(onSaved).not.toHaveBeenCalled();
  });

  it('opens the GitHub settings in place without reloading the page', async () => {
    listRepositories.mockResolvedValue(unreachableResponse());
    renderDetail(gitHubSource({ has_credentials: true }));

    fireEvent.click(screen.getByRole('button', { name: 'Switch to GitHub App' }));

    const status = await unreachableStatus();
    const followedNatively = fireEvent.click(
      within(status).getByRole('link', { name: 'Connect GitHub' })
    );
    expect(followedNatively).toBe(false);
  });

  it('announces the message in a status region that was already rendered empty', async () => {
    listRepositories.mockResolvedValue(unreachableResponse());
    renderDetail(gitHubSource({ has_credentials: true }));
    const initialStatus = connectionStatus();
    expect(initialStatus.textContent).toBe('');

    fireEvent.click(screen.getByRole('button', { name: 'Switch to GitHub App' }));

    const status = await unreachableStatus();
    expect(status).toBe(initialStatus);
  });

  it('links an owner to the GitHub settings', async () => {
    workspaceState = { currentOrganization: { id: 'org-1', name: 'Acme', role: 'owner' } };
    listRepositories.mockResolvedValue(unreachableResponse());
    renderDetail(appSource({ github_installation_status: 'revoked' }));

    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));

    const status = await unreachableStatus();
    expect(within(status).getByRole('link', { name: 'Connect GitHub' })).toBeTruthy();
    expect(updateSource).not.toHaveBeenCalled();
  });

  it('links the owner of a just-created organization, whose role is still unset', async () => {
    workspaceState = { currentOrganization: { id: 'org-1', name: 'Acme' } };
    listRepositories.mockResolvedValue(unreachableResponse());
    renderDetail(appSource({ github_installation_status: 'revoked' }));

    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));

    const status = await unreachableStatus();
    expect(within(status).getByRole('link', { name: 'Connect GitHub' })).toBeTruthy();
    expect(within(status).queryByText('Ask an organization admin to connect GitHub.')).toBeNull();
  });

  it('asks a member to contact an admin instead of linking', async () => {
    workspaceState = { currentOrganization: { id: 'org-1', name: 'Acme', role: 'member' } };
    listRepositories.mockResolvedValue(unreachableResponse());
    renderDetail(appSource({ github_installation_status: 'revoked' }));

    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));

    const status = await unreachableStatus();
    expect(within(status).getByText('Ask an organization admin to connect GitHub.')).toBeTruthy();
    expect(within(status).queryByRole('link')).toBeNull();
    expect(updateSource).not.toHaveBeenCalled();
  });

  it('offers neither the link nor the member hint while the role is resolving', async () => {
    workspaceState = { currentOrganization: { id: 'org-1', name: 'Acme' }, resolvingRole: true };
    listRepositories.mockResolvedValue(unreachableResponse());
    renderDetail(appSource({ github_installation_status: 'revoked' }));

    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));

    const status = await unreachableStatus();
    expect(within(status).queryByRole('link')).toBeNull();
    expect(within(status).queryByText('Ask an organization admin to connect GitHub.')).toBeNull();
  });

  it('shows the message for an empty search result', async () => {
    listRepositories.mockResolvedValue(repositoriesResponse([], []));
    renderDetail(gitHubSource({ github_installation_id: null, has_credentials: false }));

    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));

    const status = await unreachableStatus();
    expect(updateSource).not.toHaveBeenCalled();
  });

  it('shows the message without searching when there is no current organization', async () => {
    workspaceState = { currentOrganization: null };
    renderDetail(gitHubSource({ has_credentials: true }));

    fireEvent.click(screen.getByRole('button', { name: 'Switch to GitHub App' }));

    const status = await unreachableStatus();
    expect(within(status).getByText('Ask an organization admin to connect GitHub.')).toBeTruthy();
    expect(listRepositories).not.toHaveBeenCalled();
    expect(updateSource).not.toHaveBeenCalled();
  });
});

describe('SourceDetail connection request errors', () => {
  it('shows a failed repository search in the alert', async () => {
    listRepositories.mockRejectedValue(new Error('Failed to load repositories'));
    renderDetail(gitHubSource({ has_credentials: true }));

    fireEvent.click(screen.getByRole('button', { name: 'Switch to GitHub App' }));

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toBe('Failed to load repositories');
    expect(updateSource).not.toHaveBeenCalled();
    expect(connectionStatus().textContent).toBe('');
  });

  it('shows a failed update in the alert without calling onSaved', async () => {
    listRepositories.mockResolvedValue(
      repositoriesResponse(
        [repository('acme/api', 'installation-1')],
        [installation('installation-1')]
      )
    );
    updateSource.mockRejectedValue(new Error('Failed to update source'));
    renderDetail(gitHubSource({ has_credentials: true }));

    fireEvent.click(screen.getByRole('button', { name: 'Switch to GitHub App' }));

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toBe('Failed to update source');
    expect(onSaved).not.toHaveBeenCalled();
  });

  it('clears an earlier alert when the next attempt succeeds', async () => {
    listRepositories.mockRejectedValueOnce(new Error('Failed to load repositories'));
    listRepositories.mockResolvedValueOnce(
      repositoriesResponse(
        [repository('acme/api', 'installation-1')],
        [installation('installation-1')]
      )
    );
    renderDetail(gitHubSource({ has_credentials: true }));
    const switchButton = screen.getByRole('button', { name: 'Switch to GitHub App' });

    fireEvent.click(switchButton);
    await screen.findByRole('alert');
    fireEvent.click(switchButton);

    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole('alert')).toBeNull();
  });
});

describe('SourceDetail access token field', () => {
  it('is absent for an app-backed source', () => {
    renderDetail(appSource());

    expect(accessTokenField()).toBeNull();
  });

  it('is absent for a revoked installation so it never falls back to a token', () => {
    renderDetail(appSource({ github_installation_status: 'revoked' }));

    expect(accessTokenField()).toBeNull();
  });

  it('is present for a token-backed source', () => {
    renderDetail(gitHubSource({ github_installation_id: null, has_credentials: true }));

    expect(accessTokenField()).toBeTruthy();
  });

  it('is present for a source from an older backend without the new fields', () => {
    renderDetail(gitHubSource());

    expect(accessTokenField()).toBeTruthy();
  });
});

describe('SourceDetail field styling', () => {
  const styledInputTypes = [
    ...read(join(import.meta.dir, '..', '..', '..', 'styles', 'forms.css')).matchAll(
      /:where\(input\[type='([a-z]+)'\]\)/g
    ),
  ].map((match) => match[1]);

  it.each(['Type', 'URL', 'Name'])('gives the %s field a type the form styles match', (label) => {
    renderDetail(gitHubSource());

    expect(styledInputTypes).toContain(screen.getByLabelText(label).getAttribute('type') ?? '');
  });

  it('gives every text field a type the form styles match', () => {
    const { container } = renderDetail(gitHubSource());

    const unstyled = [...container.querySelectorAll('input:not([type="checkbox"])')]
      .filter((input) => !styledInputTypes.includes(input.getAttribute('type') ?? ''))
      .map((input) => input.id);
    expect(unstyled).toEqual([]);
  });
});
