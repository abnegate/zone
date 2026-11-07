import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { type ReactNode, useState } from 'react';
import { MemoryRouter } from 'react-router-dom';
import type { OrgRole } from '../../auth/types';
import { GITHUB_SETTINGS_PATH } from '../../github/callback';
import type {
  GitHubBranchesResponse,
  GitHubRepositoriesResponse,
  GitHubRepository,
} from '../../github/types';
import type { CreateSourceRequest, Source } from '../types';

type User = ReturnType<typeof userEvent.setup>;

const mockGetSourceTypes = mock();
const mockListRepositories = mock<() => Promise<GitHubRepositoriesResponse>>();
const mockListBranches = mock<() => Promise<GitHubBranchesResponse>>();

let role: OrgRole = 'admin';

mock.module('../../../api/sources', () => ({
  sourcesApi: {
    getSourceTypes: mockGetSourceTypes,
  },
}));

mock.module('../../../api/github', () => ({
  githubApi: {
    getConnection: mock(),
    registerApp: mock(),
    createInstallation: mock(),
    syncInstallations: mock(),
    removeInstallation: mock(),
    deleteApp: mock(),
    listRepositories: mockListRepositories,
    listBranches: mockListBranches,
  },
}));

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentWorkspace: { id: 'workspace-1', name: 'Workspace' },
    currentOrganization: { id: 'org-1', name: 'Acme', role },
    organizations: [],
    loading: false,
  }),
}));

let CreateSourceWizard: typeof import('./CreateSourceWizard').CreateSourceWizard;

beforeAll(async () => {
  ({ CreateSourceWizard } = await import('./CreateSourceWizard'));
});

afterAll(() => {
  mock.restore();
});

const kind = (id: string) => ({ id, name: id, category: 'file', enabled: true });

const offeredKinds = () =>
  Array.from(document.querySelectorAll('.source-type-option .source-type-name')).map(
    (node) => node.textContent
  );

const offeredDescriptions = () =>
  Array.from(document.querySelectorAll('.source-type-option .source-type-desc')).map(
    (node) => node.textContent
  );

const acmeApi: GitHubRepository = {
  installation_id: 'installation-1',
  id: 1,
  owner: 'acme',
  name: 'api',
  full_name: 'acme/api',
  private: true,
  archived: false,
  default_branch: 'trunk',
  html_url: 'https://github.com/acme/api',
};

const acmeWeb: GitHubRepository = {
  ...acmeApi,
  id: 2,
  name: 'web',
  full_name: 'acme/web',
  private: false,
  default_branch: 'main',
  html_url: 'https://github.com/acme/web',
};

const connectedRepositories: GitHubRepositoriesResponse = {
  repositories: [acmeApi, acmeWeb],
  installations: [
    {
      id: 'installation-1',
      account_login: 'acme',
      html_url: 'https://github.com/organizations/acme/settings/installations/1',
      error: null,
    },
  ],
  total: 2,
  next_page: null,
};

const disconnectedRepositories: GitHubRepositoriesResponse = {
  repositories: [],
  installations: [],
  total: 0,
  next_page: null,
};

const created = {
  id: 'source-zone',
  name: 'abnegate/zone',
  source_type: 'github',
  category: 'file',
  config: { owner: 'abnegate', repo: 'zone', branch: 'main' },
  url: 'https://github.com/abnegate/zone',
  description: null,
  is_active: true,
  last_verified_at: null,
  last_error: null,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
} as Source;

function withQueryClient(children: ReactNode) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  return (
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>{children}</MemoryRouter>
    </QueryClientProvider>
  );
}

function renderWizard(
  createSource: (request: CreateSourceRequest) => Promise<Source> = () =>
    Promise.reject(new Error('not under test'))
) {
  return render(
    withQueryClient(
      <CreateSourceWizard
        isOpen
        onClose={() => {}}
        onCreated={() => {}}
        createSource={createSource}
      />
    )
  );
}

function disclosure(scope: HTMLElement = document.body): HTMLElement {
  return within(scope).getByRole('button', { name: 'Use an access token instead' });
}

function repositoryCombobox(): HTMLInputElement {
  return screen.getByRole('combobox', { name: 'Repository' }) as HTMLInputElement;
}

function nextButton(): HTMLElement {
  return screen.getByRole('button', { name: 'Next' });
}

async function reachGitHubStep(user: User) {
  await waitFor(() => expect(offeredKinds()).toContain('GitHub'));
  await user.click(nextButton());
  await screen.findByRole('button', { name: 'Use an access token instead' });
}

async function pickRepository(user: User, name: RegExp) {
  await user.click(await screen.findByRole('combobox', { name: 'Repository' }));
  await user.click(await screen.findByRole('option', { name }));
  await waitFor(() => expect(nextButton()).toBeEnabled());
}

async function submit(user: User) {
  await user.click(nextButton());
  await screen.findByLabelText(/Display Name/);
  await user.click(screen.getByRole('button', { name: 'Add Source' }));
}

beforeEach(() => {
  role = 'admin';
  mockGetSourceTypes.mockReset();
  mockGetSourceTypes.mockResolvedValue([kind('github')]);
  mockListRepositories.mockReset();
  mockListRepositories.mockResolvedValue(connectedRepositories);
  mockListBranches.mockReset();
  mockListBranches.mockResolvedValue({
    branches: [
      { name: 'trunk', protected: true },
      { name: 'release', protected: false },
    ],
    next_page: null,
  });
});

describe('CreateSourceWizard kinds', () => {
  it('offers only the kinds the server lists', async () => {
    mockGetSourceTypes.mockResolvedValue([kind('github'), kind('text')]);

    renderWizard();

    await waitFor(() => {
      expect(offeredKinds()).toEqual(['GitHub', 'Text']);
    });
    expect(mockGetSourceTypes).toHaveBeenCalledTimes(1);
  });

  it('describes each kind in a phrase short enough for one line of a tile', async () => {
    mockGetSourceTypes.mockResolvedValue(
      ['github', 'gitlab', 'filesystem', 'web', 'text'].map(kind)
    );

    renderWizard();

    await waitFor(() => {
      expect(offeredDescriptions()).toEqual([
        'GitHub repository',
        'GitLab project',
        'Local directory',
        'Web page',
        'Raw text',
      ]);
    });
  });

  it('never offers a kind without an adapter, even when the server cannot be asked', async () => {
    mockGetSourceTypes.mockRejectedValue(new Error('offline'));

    renderWizard();

    await waitFor(() => {
      expect(mockGetSourceTypes).toHaveBeenCalledTimes(1);
    });
    const kinds = offeredKinds();
    expect(kinds).toEqual(['GitHub', 'GitLab', 'Filesystem', 'Web URL', 'Text']);
    expect(kinds).not.toContain('Email');
    expect(kinds).not.toContain('Calendar');
    expect(screen.queryByText('Notion')).toBeNull();
  });
});

describe('CreateSourceWizard embedding', () => {
  function Opener({ onCreated }: { onCreated: (source: Source) => void }) {
    const [open, setOpen] = useState(false);
    return (
      <>
        <button type="button" onClick={() => setOpen(true)}>
          Open
        </button>
        <CreateSourceWizard
          isOpen={open}
          onClose={() => setOpen(false)}
          onCreated={onCreated}
          createSource={() => Promise.resolve(created)}
        />
      </>
    );
  }

  it('hands the created source to onCreated', async () => {
    const user = userEvent.setup();
    const onCreated = mock((_source: Source) => {});
    render(withQueryClient(<Opener onCreated={onCreated} />));
    await user.click(screen.getByRole('button', { name: 'Open' }));
    const dialog = await screen.findByRole('dialog', { name: 'Add Source' });

    await user.click(within(dialog).getByRole('button', { name: 'Next' }));
    await user.click(
      await within(dialog).findByRole('button', { name: 'Use an access token instead' })
    );
    await user.type(await within(dialog).findByLabelText(/Owner/), 'abnegate');
    await user.type(within(dialog).getByLabelText(/Repository/), 'zone');
    await user.click(within(dialog).getByRole('button', { name: 'Next' }));
    await within(dialog).findByLabelText(/Display Name/);
    await user.click(within(dialog).getByRole('button', { name: 'Add Source' }));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(created));
  });

  it('takes focus when it opens and returns it to the opener when it closes', async () => {
    const user = userEvent.setup();
    render(withQueryClient(<Opener onCreated={() => {}} />));
    const opener = screen.getByRole('button', { name: 'Open' });
    await user.click(opener);
    const dialog = await screen.findByRole('dialog', { name: 'Add Source' });

    expect(dialog.contains(document.activeElement)).toBe(true);

    await user.keyboard('{Escape}');

    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Add Source' })).toBeNull());
    expect(document.activeElement).toBe(opener);
  });

  it('forgets the picked repository and the access token mode when it closes', async () => {
    const user = userEvent.setup();
    render(withQueryClient(<Opener onCreated={() => {}} />));
    await user.click(screen.getByRole('button', { name: 'Open' }));
    await reachGitHubStep(user);
    await pickRepository(user, /^acme\/api/);
    await user.click(disclosure());

    await user.keyboard('{Escape}');
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Add Source' })).toBeNull());
    await user.click(screen.getByRole('button', { name: 'Open' }));
    await reachGitHubStep(user);

    expect(disclosure()).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByLabelText(/Owner/)).toBeNull();
    expect(await screen.findByRole('combobox', { name: 'Repository' })).toHaveValue('');
    expect(nextButton()).toBeDisabled();
  });
});

describe('CreateSourceWizard configuration step', () => {
  it('seats the access token beside the branch and states its hint once', async () => {
    renderWizard();
    await waitFor(() => {
      expect(offeredKinds()).toEqual(['GitHub']);
    });
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Use an access token instead' }));

    const branch = await screen.findByLabelText(/Branch/);
    const token = screen.getByLabelText(/Access Token/);
    const row = branch.closest('.form-row');
    expect(row).not.toBeNull();
    expect(token.closest('.form-row')).toBe(row);
    expect(row?.querySelectorAll('.form-group')).toHaveLength(2);
    expect(screen.getAllByText('Token required for private repos and write access')).toHaveLength(
      1
    );

    fireEvent.change(token, { target: { value: 'ghp_secret' } });
    expect(token).toHaveValue('ghp_secret');
    expect(branch).toHaveValue('main');
  });
});

describe('CreateSourceWizard GitHub repository picker', () => {
  it('shows the repository picker by default and holds Next until a repository is picked', async () => {
    const user = userEvent.setup();
    renderWizard();
    await reachGitHubStep(user);

    expect(await screen.findByRole('combobox', { name: 'Repository' })).toBeInTheDocument();
    expect(disclosure()).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByLabelText(/Owner/)).toBeNull();
    expect(screen.queryByLabelText(/Access Token/)).toBeNull();
    expect(nextButton()).toBeDisabled();

    await pickRepository(user, /^acme\/api/);

    expect(repositoryCombobox()).toHaveValue('acme/api');
    expect(nextButton()).toBeEnabled();
  });

  it('prefills the branch with the default branch of the picked repository', async () => {
    const user = userEvent.setup();
    renderWizard();
    await reachGitHubStep(user);

    await pickRepository(user, /^acme\/api/);

    expect(screen.getByLabelText('Branch')).toHaveValue('trunk');
  });

  it('creates the source from the picked repository with its installation and no credentials', async () => {
    const createSource = mock((_request: CreateSourceRequest) => Promise.resolve(created));
    const user = userEvent.setup();
    renderWizard(createSource);
    await reachGitHubStep(user);
    await pickRepository(user, /^acme\/api/);

    await user.click(nextButton());
    expect(await screen.findByLabelText(/Display Name/)).toHaveAttribute('placeholder', 'acme/api');
    await user.click(screen.getByRole('button', { name: 'Add Source' }));

    await waitFor(() => expect(createSource).toHaveBeenCalledTimes(1));
    const request = createSource.mock.calls[0]?.[0] as CreateSourceRequest;
    expect(request).toEqual({
      name: 'acme/api',
      source_type: 'github',
      config: { owner: 'acme', repo: 'api', branch: 'trunk' },
      github_installation_id: 'installation-1',
      url: 'https://github.com/acme/api',
    });
    expect(Object.hasOwn(request, 'credentials')).toBe(false);
  });

  it('honours a custom branch and a custom display name', async () => {
    const createSource = mock((_request: CreateSourceRequest) => Promise.resolve(created));
    const user = userEvent.setup();
    renderWizard(createSource);
    await reachGitHubStep(user);
    await pickRepository(user, /^acme\/api/);

    const branch = screen.getByLabelText('Branch');
    await user.clear(branch);
    await user.type(branch, 'release');
    await user.click(nextButton());
    await user.type(await screen.findByLabelText(/Display Name/), 'Acme API');
    await user.click(screen.getByRole('button', { name: 'Add Source' }));

    await waitFor(() => expect(createSource).toHaveBeenCalledTimes(1));
    expect(createSource.mock.calls[0]?.[0]).toEqual({
      name: 'Acme API',
      source_type: 'github',
      config: { owner: 'acme', repo: 'api', branch: 'release' },
      github_installation_id: 'installation-1',
      url: 'https://github.com/acme/api',
    });
  });

  it('falls back to the default branch when the branch is cleared', async () => {
    const createSource = mock((_request: CreateSourceRequest) => Promise.resolve(created));
    const user = userEvent.setup();
    renderWizard(createSource);
    await reachGitHubStep(user);
    await pickRepository(user, /^acme\/api/);

    await user.clear(screen.getByLabelText('Branch'));
    await submit(user);

    await waitFor(() => expect(createSource).toHaveBeenCalledTimes(1));
    expect(createSource.mock.calls[0]?.[0]?.config).toEqual({
      owner: 'acme',
      repo: 'api',
      branch: 'trunk',
    });
  });

  it('restores the picked repository when the access token fields are opened and closed', async () => {
    const user = userEvent.setup();
    renderWizard();
    await reachGitHubStep(user);
    await pickRepository(user, /^acme\/api/);

    await user.click(disclosure());

    expect(disclosure()).toHaveAttribute('aria-expanded', 'true');
    expect(screen.queryByRole('combobox', { name: 'Repository' })).toBeNull();
    expect(await screen.findByLabelText(/Owner/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Access Token/)).toBeInTheDocument();

    await user.click(disclosure());

    expect(disclosure()).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByLabelText(/Owner/)).toBeNull();
    expect(await screen.findByRole('combobox', { name: 'Repository' })).toHaveValue('acme/api');
    expect(nextButton()).toBeEnabled();
  });

  it('offers an admin the Connect GitHub link when no GitHub account is connected', async () => {
    mockListRepositories.mockResolvedValue(disconnectedRepositories);
    const user = userEvent.setup();
    renderWizard();
    await reachGitHubStep(user);

    expect(await screen.findByText('No GitHub account is connected.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Connect GitHub' })).toHaveAttribute(
      'href',
      GITHUB_SETTINGS_PATH
    );
    expect(nextButton()).toBeDisabled();
  });

  it('hides the Connect GitHub link from a member when no GitHub account is connected', async () => {
    role = 'member';
    mockListRepositories.mockResolvedValue(disconnectedRepositories);
    const user = userEvent.setup();
    renderWizard();
    await reachGitHubStep(user);

    expect(await screen.findByText('No GitHub account is connected.')).toBeInTheDocument();
    expect(screen.getByText('Ask an organization admin to connect GitHub.')).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Connect GitHub' })).toBeNull();
    expect(disclosure()).toBeInTheDocument();
  });
});
