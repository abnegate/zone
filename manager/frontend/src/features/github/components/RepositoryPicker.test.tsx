import { afterAll, beforeAll, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { type ComponentType, useState } from 'react';
import type { GitHubRepositorySearch } from '../../../api/github';
import type {
  GitHubRepositoriesResponse,
  GitHubRepository,
  GitHubRepositoryInstallation,
} from '../types';

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

interface RepositoryPickerProps {
  organizationId: string | null;
  canManage: boolean;
  value: GitHubRepository | null;
  onChange: (repository: GitHubRepository | null) => void;
}

type User = ReturnType<typeof userEvent.setup>;

let RepositoryPicker: ComponentType<RepositoryPickerProps>;

beforeAll(async () => {
  const module = (await import('./RepositoryPicker')) as {
    RepositoryPicker?: ComponentType<RepositoryPickerProps>;
    default?: ComponentType<RepositoryPickerProps>;
  };
  const component = module.RepositoryPicker ?? module.default;
  if (component === undefined) {
    throw new Error('RepositoryPicker.tsx exports no RepositoryPicker component');
  }
  RepositoryPicker = component;
});

afterAll(() => {
  mock.restore();
});

let nextRepositoryId = 1;

function repository(
  owner: string,
  name: string,
  overrides: Partial<GitHubRepository> = {}
): GitHubRepository {
  return {
    installation_id: owner === 'acme' ? 'installation-1' : 'installation-2',
    id: nextRepositoryId++,
    owner,
    name,
    full_name: `${owner}/${name}`,
    private: false,
    archived: false,
    default_branch: 'main',
    html_url: `https://github.com/${owner}/${name}`,
    ...overrides,
  };
}

function installation(
  login: string,
  overrides: Partial<GitHubRepositoryInstallation> = {}
): GitHubRepositoryInstallation {
  return {
    id: login === 'acme' ? 'installation-1' : 'installation-2',
    account_login: login,
    html_url: `https://github.com/organizations/${login}/settings/installations/1`,
    error: null,
    ...overrides,
  };
}

function page(
  repositories: GitHubRepository[],
  overrides: Partial<GitHubRepositoriesResponse> = {}
): GitHubRepositoriesResponse {
  return {
    repositories,
    installations: [installation('acme'), installation('abnegate')],
    total: repositories.length,
    next_page: null,
    ...overrides,
  };
}

function deferred<Value>() {
  let resolve: (value: Value) => void = () => {};
  const promise = new Promise<Value>((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
}

const api = repository('acme', 'api');
const web = repository('acme', 'web', { private: true });
const legacy = repository('acme', 'legacy', { archived: true });
const zone = repository('abnegate', 'zone', { private: true });

const firstPage = page([api, web, legacy, zone]);

interface RenderOptions {
  organizationId?: string | null;
  canManage?: boolean;
}

function renderPicker({ organizationId = 'org-1', canManage = true }: RenderOptions = {}) {
  const onChange = mock<(repository: GitHubRepository | null) => void>();
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });

  function Harness() {
    const [value, setValue] = useState<GitHubRepository | null>(null);
    return (
      <RepositoryPicker
        organizationId={organizationId}
        canManage={canManage}
        value={value}
        onChange={(repository) => {
          setValue(repository);
          onChange(repository);
        }}
      />
    );
  }

  const user = userEvent.setup();
  render(
    <QueryClientProvider client={queryClient}>
      <Harness />
    </QueryClientProvider>
  );
  return { user, onChange };
}

function combobox(): HTMLInputElement {
  return screen.getByRole('combobox', { name: 'Repository' }) as HTMLInputElement;
}

async function openPopup(user: User) {
  const input = combobox();
  if (input.getAttribute('aria-expanded') !== 'true') {
    await user.click(input);
  }
  if (input.getAttribute('aria-expanded') !== 'true') {
    await user.keyboard('{ArrowDown}');
  }
  await waitFor(() => expect(input).toHaveAttribute('aria-expanded', 'true'));
}

async function waitForOptions(user: User) {
  await openPopup(user);
  await screen.findByRole('option', { name: /^acme\/api/ });
}

function queries(): string[] {
  return listRepositories.mock.calls.map(([, search]) => search?.query ?? '');
}

function option(name: RegExp): HTMLElement {
  return screen.getByRole('option', { name });
}

beforeEach(() => {
  listRepositories.mockReset();
  listRepositories.mockImplementation(async () => firstPage);
});

describe('RepositoryPicker fetching and debounce', () => {
  it('fetches the first page with an empty query on mount', async () => {
    renderPicker();

    await waitFor(() =>
      expect(listRepositories).toHaveBeenCalledWith(
        'org-1',
        expect.objectContaining({ query: '', page: 1, perPage: 50 })
      )
    );
  });

  it('sends one search for text typed quickly and none for the partial text', async () => {
    const { user } = renderPicker();
    await waitForOptions(user);

    await user.type(combobox(), 'zon');

    await waitFor(() => expect(queries()).toContain('zon'));
    expect(queries().filter((query) => query === 'zon')).toHaveLength(1);
    expect(queries()).not.toContain('z');
    expect(queries()).not.toContain('zo');
    expect(listRepositories).toHaveBeenLastCalledWith(
      'org-1',
      expect.objectContaining({ query: 'zon', page: 1 })
    );
  });

  it('keeps the combobox focused and the earlier options visible while a search is pending', async () => {
    const search = deferred<GitHubRepositoriesResponse>();
    listRepositories.mockImplementation(async (_organizationId, parameters) =>
      parameters?.query === 'zon' ? search.promise : firstPage
    );
    const { user } = renderPicker();
    await waitForOptions(user);

    await user.type(combobox(), 'zon');
    await waitFor(() => expect(queries()).toContain('zon'));

    const input = combobox();
    expect(document.activeElement).toBe(input);
    expect(input).toHaveValue('zon');
    expect(option(/^acme\/api/)).toBeInTheDocument();
    expect(option(/^abnegate\/zone/)).toBeInTheDocument();

    search.resolve(page([zone]));

    await waitFor(() =>
      expect(screen.queryByRole('option', { name: /^acme\/api/ })).not.toBeInTheDocument()
    );
    expect(option(/^abnegate\/zone/)).toBeInTheDocument();
    expect(document.activeElement).toBe(combobox());
  });

  it('says nothing matched when a search returns no repositories', async () => {
    listRepositories.mockImplementation(async (_organizationId, parameters) =>
      parameters?.query === 'nothing' ? page([]) : firstPage
    );
    const { user } = renderPicker();
    await waitForOptions(user);

    await user.type(combobox(), 'nothing');

    expect(await screen.findByText('No repositories match “nothing”.')).toBeInTheDocument();
    expect(screen.queryByRole('option')).not.toBeInTheDocument();
  });
});

describe('RepositoryPicker listbox and selection', () => {
  it('groups options by account in API order', async () => {
    const { user } = renderPicker();
    await waitForOptions(user);

    const listbox = screen.getByRole('listbox', { name: 'Repositories' });
    const groups = within(listbox).getAllByRole('group');
    expect(groups.map((group) => group.getAttribute('aria-label'))).toEqual(['acme', 'abnegate']);

    const acme = within(listbox).getByRole('group', { name: 'acme' });
    expect(within(acme).getAllByRole('option')).toHaveLength(3);
    expect(within(acme).getByRole('option', { name: /^acme\/api/ })).toBeInTheDocument();
    expect(within(acme).getByRole('option', { name: /^acme\/web/ })).toBeInTheDocument();
    expect(within(acme).getByRole('option', { name: /^acme\/legacy/ })).toBeInTheDocument();

    const abnegate = within(listbox).getByRole('group', { name: 'abnegate' });
    expect(within(abnegate).getAllByRole('option')).toHaveLength(1);
    expect(within(abnegate).getByRole('option', { name: /^abnegate\/zone/ })).toBeInTheDocument();
  });

  it('shows the private lock only inside private options', async () => {
    const { user } = renderPicker();
    await waitForOptions(user);

    expect(screen.getAllByRole('img', { name: 'Private' })).toHaveLength(2);
    for (const name of [/^acme\/web/, /^abnegate\/zone/]) {
      expect(within(option(name)).getByRole('img', { name: 'Private' })).toBeInTheDocument();
    }
    for (const name of [/^acme\/api/, /^acme\/legacy/]) {
      expect(within(option(name)).queryByRole('img', { name: 'Private' })).not.toBeInTheDocument();
    }
  });

  it('marks archived options disabled and ignores clicks on them', async () => {
    const { user, onChange } = renderPicker();
    await waitForOptions(user);

    const archived = option(/^acme\/legacy/);
    expect(archived).toHaveAttribute('aria-disabled', 'true');
    expect(within(archived).getByText('Archived')).toBeInTheDocument();
    expect(option(/^acme\/api/)).not.toHaveAttribute('aria-disabled', 'true');

    await user.click(archived);

    expect(onChange).not.toHaveBeenCalled();
    expect(combobox()).toHaveValue('');
  });

  it('selects a clicked option, fills the input and closes the popup', async () => {
    const { user, onChange } = renderPicker();
    await waitForOptions(user);

    await user.click(option(/^acme\/web/));

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith(web);
    expect(combobox()).toHaveValue('acme/web');
    expect(combobox()).toHaveAttribute('aria-expanded', 'false');
    expect(document.activeElement).toBe(combobox());
  });

  it('moves the active option with the arrow keys and selects it with Enter', async () => {
    const { user, onChange } = renderPicker();
    await waitForOptions(user);
    const input = combobox();

    await user.keyboard('{ArrowDown}');
    const firstActive = input.getAttribute('aria-activedescendant') ?? '';
    expect(firstActive).not.toBe('');
    expect(document.getElementById(firstActive)).toHaveAttribute('role', 'option');

    await user.keyboard('{ArrowDown}');
    const secondActive = input.getAttribute('aria-activedescendant') ?? '';
    expect(secondActive).not.toBe('');
    expect(secondActive).not.toBe(firstActive);
    expect(document.getElementById(secondActive)).toHaveAttribute('role', 'option');

    await user.keyboard('{ArrowUp}');
    expect(input).toHaveAttribute('aria-activedescendant', firstActive);

    const activeText = document.getElementById(firstActive)?.textContent ?? '';
    const expected = [api, web, legacy, zone].find((candidate) =>
      activeText.startsWith(candidate.full_name)
    );
    expect(expected).toBeDefined();

    await user.keyboard('{Enter}');

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith(expected ?? null);
    expect(input).toHaveValue(expected?.full_name ?? '');
    expect(input).toHaveAttribute('aria-expanded', 'false');
  });

  it('closes the popup on Escape', async () => {
    const { user } = renderPicker();
    await waitForOptions(user);

    await user.keyboard('{Escape}');

    expect(combobox()).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('option', { name: /^acme\/api/ })).not.toBeInTheDocument();
  });

  it('keeps an Escape that closes the popup from reaching document listeners', async () => {
    const documentEscape = mock((_event: KeyboardEvent) => {});
    document.addEventListener('keydown', documentEscape);
    try {
      const { user } = renderPicker();
      await waitForOptions(user);

      await user.keyboard('{Escape}');

      expect(combobox()).toHaveAttribute('aria-expanded', 'false');
      expect(documentEscape).not.toHaveBeenCalled();

      await user.keyboard('{Escape}');

      expect(documentEscape).toHaveBeenCalledTimes(1);
    } finally {
      document.removeEventListener('keydown', documentEscape);
    }
  });

  it('reopens the popup on ArrowDown after Escape', async () => {
    const { user } = renderPicker();
    await waitForOptions(user);
    await user.keyboard('{Escape}');

    await user.keyboard('{ArrowDown}');

    expect(combobox()).toHaveAttribute('aria-expanded', 'true');
    expect(option(/^acme\/api/)).toBeInTheDocument();
  });

  it('lists every repository again when reopened after selecting a searched one', async () => {
    listRepositories.mockImplementation(async (_organizationId, parameters) =>
      parameters?.query === 'zon' ? page([zone]) : firstPage
    );
    const { user } = renderPicker();
    await waitForOptions(user);
    await user.type(combobox(), 'zon');
    await waitFor(() =>
      expect(screen.queryByRole('option', { name: /^acme\/api/ })).not.toBeInTheDocument()
    );

    await user.click(option(/^abnegate\/zone/));
    expect(combobox()).toHaveValue('abnegate/zone');
    await openPopup(user);

    expect(await screen.findByRole('option', { name: /^acme\/api/ })).toBeInTheDocument();
    expect(option(/^abnegate\/zone/)).toBeInTheDocument();
    expect(queries().at(-1)).toBe('');
  });

  it('points aria-controls at the listbox only while it is rendered', async () => {
    const { user } = renderPicker();
    await waitFor(() => expect(listRepositories).toHaveBeenCalled());
    expect(combobox()).not.toHaveAttribute('aria-controls');

    await waitForOptions(user);

    const controls = combobox().getAttribute('aria-controls') ?? '';
    expect(document.getElementById(controls)).toBe(
      screen.getByRole('listbox', { name: 'Repositories' })
    );

    await user.keyboard('{Escape}');

    expect(combobox()).not.toHaveAttribute('aria-controls');
  });

  it('scrolls the opened popup and then the active option into view', async () => {
    const scrolled: [Element, ScrollIntoViewOptions | boolean | undefined][] = [];
    function record(this: Element, options?: ScrollIntoViewOptions | boolean) {
      scrolled.push([this, options]);
    }
    const scrollIntoView = spyOn(Element.prototype, 'scrollIntoView').mockImplementation(record);
    try {
      const { user } = renderPicker();
      await waitForOptions(user);
      const popup = screen.getByRole('listbox', { name: 'Repositories' }).parentElement;
      expect(scrolled).toContainEqual([popup, { block: 'nearest' }]);

      await user.keyboard('{ArrowUp}');

      const activeId = combobox().getAttribute('aria-activedescendant') ?? '';
      const active = document.getElementById(activeId);
      expect(active).toHaveTextContent('abnegate/zone');
      expect(scrolled.at(-1)).toEqual([active, { block: 'nearest' }]);
    } finally {
      scrollIntoView.mockRestore();
    }
  });

  it('clears the selection when the user types after selecting', async () => {
    const { user, onChange } = renderPicker();
    await waitForOptions(user);
    await user.click(option(/^acme\/api/));
    expect(onChange).toHaveBeenLastCalledWith(api);

    await user.type(combobox(), 'x');

    expect(onChange).toHaveBeenLastCalledWith(null);
  });
});

describe('RepositoryPicker popup controls', () => {
  it('requests the next page with Load more and appends its repositories', async () => {
    const docs = repository('acme', 'docs');
    listRepositories.mockImplementation(async (_organizationId, parameters) =>
      parameters?.page === 2
        ? page([docs])
        : page([api, web, legacy, zone], { total: 5, next_page: 2 })
    );
    const { user } = renderPicker();
    await waitForOptions(user);

    await user.click(screen.getByRole('button', { name: 'Load more' }));

    await waitFor(() =>
      expect(listRepositories).toHaveBeenCalledWith(
        'org-1',
        expect.objectContaining({ query: '', page: 2 })
      )
    );
    await openPopup(user);
    expect(await screen.findByRole('option', { name: /^acme\/docs/ })).toBeInTheDocument();
    expect(option(/^acme\/api/)).toBeInTheDocument();
    expect(option(/^abnegate\/zone/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('hides Load more when there is no next page', async () => {
    const { user } = renderPicker();
    await waitForOptions(user);

    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('refreshes the list from GitHub and replaces the options', async () => {
    const fresh = repository('acme', 'fresh');
    listRepositories.mockImplementation(async (_organizationId, parameters) =>
      parameters?.refresh ? page([fresh]) : firstPage
    );
    const { user } = renderPicker();
    await waitForOptions(user);

    await user.click(screen.getByRole('button', { name: 'Refresh list' }));

    await waitFor(() =>
      expect(listRepositories).toHaveBeenCalledWith(
        'org-1',
        expect.objectContaining({ page: 1, refresh: true })
      )
    );
    await openPopup(user);
    expect(await screen.findByRole('option', { name: /^acme\/fresh/ })).toBeInTheDocument();
    expect(screen.queryByRole('option', { name: /^acme\/api/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('option', { name: /^abnegate\/zone/ })).not.toBeInTheDocument();
  });

  it('explains each installation error on its own line', async () => {
    listRepositories.mockImplementation(async () =>
      page([api], {
        installations: [
          installation('acme'),
          installation('revoked-org', { id: 'installation-3', error: 'revoked' }),
          installation('suspended-org', { id: 'installation-4', error: 'suspended' }),
          installation('offline-org', { id: 'installation-5', error: 'unavailable' }),
        ],
      })
    );
    const { user } = renderPicker();
    await waitForOptions(user);

    expect(screen.getByText('GitHub access for revoked-org was revoked.')).toBeInTheDocument();
    expect(screen.getByText('GitHub access for suspended-org is suspended.')).toBeInTheDocument();
    expect(screen.getByText(/^Couldn['’]t reach GitHub for offline-org\.$/)).toBeInTheDocument();
    expect(screen.queryByText(/GitHub access for acme/)).not.toBeInTheDocument();
    expect(screen.queryByText(/reach GitHub for acme/)).not.toBeInTheDocument();
  });

  it('drops a revoked installation error once the same account is installed again', async () => {
    listRepositories.mockImplementation(async () =>
      page([api], {
        installations: [
          installation('acme', { id: 'installation-old', error: 'revoked' }),
          installation('acme'),
          installation('offline-org', { id: 'installation-5', error: 'unavailable' }),
        ],
      })
    );
    const { user } = renderPicker();
    await waitForOptions(user);

    expect(screen.queryByText('GitHub access for acme was revoked.')).not.toBeInTheDocument();
    expect(screen.getByText(/^Couldn['’]t reach GitHub for offline-org\.$/)).toBeInTheDocument();
    expect(option(/^acme\/api/)).toBeInTheDocument();
  });

  it('announces a failed refresh as an alert', async () => {
    listRepositories.mockImplementation(async (_organizationId, parameters) => {
      if (parameters?.refresh) {
        throw new Error('GitHub is down');
      }
      return firstPage;
    });
    const { user } = renderPicker();
    await waitForOptions(user);
    const alert = screen.getByRole('alert');
    expect(alert.textContent).toBe('');

    await user.click(screen.getByRole('button', { name: 'Refresh list' }));

    await waitFor(() => expect(alert).toHaveTextContent(/^Couldn['’]t refresh repositories\.$/));
    expect(screen.getByRole('alert')).toBe(alert);
  });

  it('links a single healthy installation as Adjust access on GitHub', async () => {
    const acme = installation('acme');
    listRepositories.mockImplementation(async () =>
      page([api], {
        installations: [
          acme,
          installation('revoked-org', { id: 'installation-3', error: 'revoked' }),
        ],
      })
    );
    const { user } = renderPicker();
    await waitForOptions(user);

    expect(screen.getByText(/Can['’]t find a repo\?/)).toBeInTheDocument();
    const link = screen.getByRole('link', { name: 'Adjust access on GitHub' });
    expect(link).toHaveAttribute('href', acme.html_url);
    expect(link).toHaveAttribute('target', '_blank');
    expect(screen.queryByRole('link', { name: /^Adjust access to/ })).not.toBeInTheDocument();
  });

  it('links each healthy installation by account when there are several', async () => {
    const acme = installation('acme');
    const abnegate = installation('abnegate');
    listRepositories.mockImplementation(async () =>
      page([api, zone], {
        installations: [
          acme,
          abnegate,
          installation('revoked-org', { id: 'installation-3', error: 'revoked' }),
        ],
      })
    );
    const { user } = renderPicker();
    await waitForOptions(user);

    expect(screen.getByText(/Can['’]t find a repo\?/)).toBeInTheDocument();
    const acmeLink = screen.getByRole('link', { name: 'Adjust access to acme on GitHub' });
    const abnegateLink = screen.getByRole('link', { name: 'Adjust access to abnegate on GitHub' });
    expect(acmeLink).toHaveAttribute('href', acme.html_url);
    expect(acmeLink).toHaveAttribute('target', '_blank');
    expect(abnegateLink).toHaveAttribute('href', abnegate.html_url);
    expect(abnegateLink).toHaveAttribute('target', '_blank');
    expect(
      screen.queryByRole('link', { name: 'Adjust access to revoked-org on GitHub' })
    ).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Adjust access on GitHub' })).not.toBeInTheDocument();
  });
});

describe('RepositoryPicker empty and error states', () => {
  it('shows the empty state when no installation is connected', async () => {
    listRepositories.mockImplementation(async () => page([], { installations: [] }));
    renderPicker();

    expect(await screen.findByText('No GitHub account is connected.')).toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: 'Repository' })).not.toBeInTheDocument();
  });

  it('shows the empty state without calling the API when there is no organization', async () => {
    renderPicker({ organizationId: null });

    expect(await screen.findByText('No GitHub account is connected.')).toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: 'Repository' })).not.toBeInTheDocument();
    expect(listRepositories).not.toHaveBeenCalled();
  });

  it('links admins to the GitHub settings from the empty state', async () => {
    listRepositories.mockImplementation(async () => page([], { installations: [] }));
    renderPicker({ canManage: true });

    const link = await screen.findByRole('link', { name: 'Connect GitHub' });
    expect(link).toHaveAttribute('href', '/org-settings?tab=github');
    expect(
      screen.queryByText('Ask an organization admin to connect GitHub.')
    ).not.toBeInTheDocument();
  });

  it('tells members to ask an admin instead of linking', async () => {
    renderPicker({ organizationId: null, canManage: false });

    expect(
      await screen.findByText('Ask an organization admin to connect GitHub.')
    ).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Connect GitHub' })).not.toBeInTheDocument();
  });

  it('shows the combobox and a loading status instead of the empty state while loading', async () => {
    listRepositories.mockImplementation(() => new Promise(() => {}));
    const { user } = renderPicker();
    await waitFor(() => expect(listRepositories).toHaveBeenCalled());

    expect(screen.queryByText('No GitHub account is connected.')).not.toBeInTheDocument();
    await user.click(combobox());

    const loading = await screen.findByText('Loading repositories…');
    expect(loading.closest('[role="status"]')).not.toBeNull();
    expect(screen.queryByText('No GitHub account is connected.')).not.toBeInTheDocument();
  });

  it('keeps the loading status region mounted and swaps only its text', async () => {
    const response = deferred<GitHubRepositoriesResponse>();
    listRepositories.mockImplementation(() => response.promise);
    renderPicker();

    const status = await screen.findByRole('status');
    await waitFor(() => expect(status).toHaveTextContent('Loading repositories…'));

    response.resolve(firstPage);

    await waitFor(() => expect(status.textContent).toBe(''));
    expect(screen.getByRole('status')).toBe(status);
  });

  it('shows the error instead of the empty state and refetches on Retry', async () => {
    listRepositories.mockImplementation(async () => {
      throw new Error('Failed to load repositories');
    });
    const { user } = renderPicker();
    await waitFor(() => expect(listRepositories).toHaveBeenCalledTimes(1));
    await user.click(combobox());

    expect(await screen.findByText(/^Couldn['’]t load repositories\./)).toBeInTheDocument();
    expect(screen.queryByText('No GitHub account is connected.')).not.toBeInTheDocument();

    listRepositories.mockImplementation(async () => firstPage);
    await user.click(screen.getByRole('button', { name: 'Retry' }));

    await waitFor(() => expect(listRepositories).toHaveBeenCalledTimes(2));
    await openPopup(user);
    expect(await screen.findByRole('option', { name: /^acme\/api/ })).toBeInTheDocument();
    expect(screen.queryByText(/^Couldn['’]t load repositories\./)).not.toBeInTheDocument();
  });
});
