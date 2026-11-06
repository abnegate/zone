import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { type ReactNode, useState } from 'react';
import type { GitHubRepositorySearch } from '../../../api/github';
import type { GitHubRepositoriesResponse, GitHubRepository } from '../../github';
import type { CreateSourceRequest, Source } from '../types';
import type { GitHubConnectionMode as Mode } from './GitHubConnectionMode';

type Role = 'owner' | 'admin' | 'member';

const listRepositories =
  mock<(organizationId: string, search?: GitHubRepositorySearch) => Promise<unknown>>();
const listBranches = mock(() => Promise.resolve({ branches: [], next_page: null }));
const getSourceTypes = mock(() =>
  Promise.resolve([{ id: 'github', name: 'GitHub', category: 'file', enabled: true }])
);

let role: Role = 'admin';

mock.module('../../../api/github', () => ({
  githubApi: {
    getConnection: mock(),
    registerApp: mock(),
    createInstallation: mock(),
    syncInstallations: mock(),
    removeInstallation: mock(),
    deleteApp: mock(),
    listRepositories,
    listBranches,
  },
}));

mock.module('../../../api/sources', () => ({
  sourcesApi: { getSourceTypes },
}));

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    organizations: [],
    currentOrganization: { id: 'org-1', name: 'Acme', role },
    loading: false,
    setCurrentOrganization: () => {},
  }),
}));

let GitHubConfiguration: typeof import('./GitHubConfiguration').GitHubConfiguration;
let GitHubConnectionMode: typeof import('./GitHubConnectionMode').GitHubConnectionMode;
let CreateSourceWizard: typeof import('./CreateSourceWizard').CreateSourceWizard;

beforeAll(async () => {
  ({ GitHubConfiguration } = await import('./GitHubConfiguration'));
  ({ GitHubConnectionMode } = await import('./GitHubConnectionMode'));
  ({ CreateSourceWizard } = await import('./CreateSourceWizard'));
});

afterAll(() => {
  mock.restore();
});

const api: GitHubRepository = {
  installation_id: 'installation-1',
  id: 1,
  owner: 'acme',
  name: 'api',
  full_name: 'acme/api',
  private: false,
  archived: false,
  default_branch: 'trunk',
  html_url: 'https://github.com/acme/api',
};

function page(repositories: GitHubRepository[]): GitHubRepositoriesResponse {
  return {
    repositories,
    installations:
      repositories.length > 0
        ? [
            {
              id: 'installation-1',
              account_login: 'acme',
              html_url: 'https://github.com/organizations/acme/settings/installations/1',
              error: null,
            },
          ]
        : [],
    total: repositories.length,
    next_page: null,
  };
}

function withQueryClient(children: ReactNode) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
}

function Harness() {
  const [mode, setMode] = useState<Mode>(GitHubConnectionMode.App);
  const [repository, setRepository] = useState<GitHubRepository | null>(null);
  const [branch, setBranch] = useState('');
  return (
    <GitHubConfiguration
      mode={mode}
      onModeChange={setMode}
      repository={repository}
      onRepositoryChange={(selected) => {
        setRepository(selected);
        setBranch(selected?.default_branch ?? '');
      }}
      branch={branch}
      onBranchChange={setBranch}
    >
      <label>
        Access Token
        <input type="password" />
      </label>
    </GitHubConfiguration>
  );
}

const disclosure = () => screen.getByRole('button', { name: 'Use an access token instead' });
const picker = () => screen.queryByRole('combobox', { name: 'Repository' });
const controlledRegion = () =>
  document.getElementById(disclosure().getAttribute('aria-controls') ?? '');

async function pickRepository(user: ReturnType<typeof userEvent.setup>, name: RegExp) {
  await user.click(await screen.findByRole('combobox', { name: 'Repository' }));
  await user.click(await screen.findByRole('option', { name }));
}

describe('GitHubConfiguration', () => {
  beforeEach(() => {
    role = 'admin';
    listRepositories.mockReset();
    listRepositories.mockResolvedValue(page([api]));
  });

  it('offers the repository picker and branch with the token fields collapsed', async () => {
    render(withQueryClient(<Harness />));

    expect(await screen.findByRole('combobox', { name: 'Repository' })).toBeTruthy();
    expect(screen.getByLabelText('Branch')).toBeTruthy();
    expect(disclosure().getAttribute('aria-expanded')).toBe('false');
    expect(controlledRegion()?.hidden).toBe(true);
    expect(screen.queryByLabelText('Access Token')).toBeNull();
  });

  it('swaps the picker for the token fields without relabelling the disclosure', async () => {
    const user = userEvent.setup();
    render(withQueryClient(<Harness />));
    await screen.findByRole('combobox', { name: 'Repository' });

    await user.click(disclosure());

    expect(disclosure().getAttribute('aria-expanded')).toBe('true');
    const region = controlledRegion();
    expect(region?.hidden).toBe(false);
    expect(within(region as HTMLElement).getByLabelText('Access Token')).toBeTruthy();
    expect(picker()).toBeNull();
    expect(screen.queryByLabelText('Branch')).toBeNull();
  });

  it('restores the picked repository and branch when the token fields collapse', async () => {
    const user = userEvent.setup();
    render(withQueryClient(<Harness />));
    await pickRepository(user, /^acme\/api/);
    expect((screen.getByLabelText('Branch') as HTMLInputElement).value).toBe('trunk');

    await user.click(disclosure());
    await user.click(disclosure());

    expect((picker() as HTMLInputElement).value).toBe('acme/api');
    expect((screen.getByLabelText('Branch') as HTMLInputElement).value).toBe('trunk');
    expect(disclosure().getAttribute('aria-expanded')).toBe('false');
  });

  it('offers Connect GitHub to an admin when nothing is connected', async () => {
    listRepositories.mockResolvedValue(page([]));
    render(withQueryClient(<Harness />));

    expect(await screen.findByRole('link', { name: 'Connect GitHub' })).toBeTruthy();
  });

  it('asks a member to find an admin instead of offering Connect GitHub', async () => {
    role = 'member';
    listRepositories.mockResolvedValue(page([]));
    render(withQueryClient(<Harness />));

    expect(await screen.findByText('Ask an organization admin to connect GitHub.')).toBeTruthy();
    expect(screen.queryByRole('link', { name: 'Connect GitHub' })).toBeNull();
  });
});

describe('CreateSourceWizard GitHub step', () => {
  beforeEach(() => {
    role = 'admin';
    listRepositories.mockReset();
    listRepositories.mockResolvedValue(page([api]));
  });

  async function openConfiguration(
    onClose: () => void = () => {},
    createSource: (request: CreateSourceRequest) => Promise<Source> = () =>
      Promise.reject(new Error('not under test'))
  ) {
    const user = userEvent.setup();
    render(
      withQueryClient(
        <CreateSourceWizard
          isOpen
          onClose={onClose}
          onCreated={() => {}}
          createSource={createSource}
        />
      )
    );
    const dialog = await screen.findByRole('dialog', { name: 'Add Source' });
    await user.click(within(dialog).getByRole('button', { name: 'Next' }));
    await within(dialog).findByRole('combobox', { name: 'Repository' });
    return { user, dialog };
  }

  it('closes only the repository popup on Escape, then the dialog on the next one', async () => {
    const onClose = mock(() => {});
    const { user, dialog } = await openConfiguration(onClose);

    await user.click(within(dialog).getByRole('combobox', { name: 'Repository' }));
    await within(dialog).findByRole('listbox', { name: 'Repositories' });

    await user.keyboard('{Escape}');

    await waitFor(() =>
      expect(within(dialog).queryByRole('listbox', { name: 'Repositories' })).toBeNull()
    );
    expect(onClose).not.toHaveBeenCalled();

    await user.keyboard('{Escape}');

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('holds Next until a repository is picked', async () => {
    const { user, dialog } = await openConfiguration();
    const next = within(dialog).getByRole('button', { name: 'Next' }) as HTMLButtonElement;

    expect(next.disabled).toBe(true);

    await pickRepository(user, /^acme\/api/);

    expect(next.disabled).toBe(false);
  });

  it('creates the source through the installation without sending credentials', async () => {
    const created = { id: 'source-1' } as Source;
    const createSource = mock((_request: CreateSourceRequest) => Promise.resolve(created));
    const { user, dialog } = await openConfiguration(() => {}, createSource);

    await pickRepository(user, /^acme\/api/);
    await user.click(within(dialog).getByRole('button', { name: 'Next' }));
    const displayName = await within(dialog).findByLabelText(/Display Name/);
    expect(displayName.getAttribute('placeholder')).toBe('acme/api');
    await user.click(within(dialog).getByRole('button', { name: 'Add Source' }));

    await waitFor(() => expect(createSource).toHaveBeenCalledTimes(1));
    const request = createSource.mock.calls[0]?.[0];
    expect(request).toStrictEqual({
      name: 'acme/api',
      source_type: 'github',
      config: { owner: 'acme', repo: 'api', branch: 'trunk' },
      github_installation_id: 'installation-1',
      url: 'https://github.com/acme/api',
    });
    expect(request && 'credentials' in request).toBe(false);
  });
});
