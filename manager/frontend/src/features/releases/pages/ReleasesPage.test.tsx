import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type React from 'react';
import { MemoryRouter } from 'react-router-dom';
import { listing, release, SOURCE_ID } from '../../../test/releases';
import { chooseSelect } from '../../../test/select';
import {
  CONNECT_SOURCE,
  NO_SOURCES_TITLE,
  NOTICES_LABEL,
  type SourceStatus,
  UNAVAILABLE,
} from '../../github';
import type { Release, ReleasesResponse } from '../types';

const mockRefresh = mock();

interface ReleasesState {
  listing: ReleasesResponse | undefined;
  loading: boolean;
  error: string | null;
  unavailable: boolean;
  refreshing: boolean;
}

const IDLE: ReleasesState = {
  listing: undefined,
  loading: false,
  error: null,
  unavailable: false,
  refreshing: false,
};

let state: ReleasesState = IDLE;

mock.module('../hooks/useReleases', () => ({
  useReleases: () => ({
    ...state,
    releases: state.listing?.releases ?? [],
    refresh: mockRefresh,
  }),
}));

mock.module('../hooks/useReleasePipeline', () => ({
  useReleasePipeline: () => ({ watching: false }),
}));

mock.module('../hooks/useReleaseOptions', () => ({
  useReleaseOptions: () => ({
    options: undefined,
    sources: [],
    loading: false,
    error: null,
    reload: mock(),
    reloading: false,
  }),
}));

mock.module('../hooks/useCreateRelease', () => ({
  useCreateRelease: () => ({
    preview: mock(),
    previewing: false,
    create: mock(),
    creating: false,
    reset: mock(),
  }),
}));

let permissions: string[] = ['releases:read', 'releases:create'];

mock.module('../../../features/auth', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    permissions,
    hasPermission: (permission: string) => permissions.includes(permission),
    hasAnyPermission: (wanted: string[]) => wanted.some((item) => permissions.includes(item)),
    hasAllPermissions: (wanted: string[]) => wanted.every((item) => permissions.includes(item)),
  }),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

let writable = true;

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentWorkspace: { id: 'workspace-1', name: 'Engineering' },
    loading: false,
    writable,
  }),
}));

let page: typeof import('./ReleasesPage');

beforeAll(async () => {
  page = await import('./ReleasesPage');
});

afterAll(() => {
  mock.restore();
});

const SECOND_SOURCE = 'second-source';

function status(sourceId: string, repository: string): SourceStatus {
  return {
    source_id: sourceId,
    name: repository,
    repository,
    repository_url: `https://github.com/${repository}`,
    freshness: 'cached',
    generated_at: '2026-11-13T09:06:00Z',
    warning: null,
  };
}

const SOURCES = [status(SOURCE_ID, 'acme/zone'), status(SECOND_SOURCE, 'acme/console')];

function recent(hoursAgo: number, overrides: Partial<Release> = {}): Release {
  return release({
    published_at: new Date(Date.now() - hoursAgo * 3_600_000).toISOString(),
    ...overrides,
  });
}

function onDay(daysAgo: number, hour: number, overrides: Partial<Release> = {}): Release {
  const date = new Date();
  date.setDate(date.getDate() - daysAgo);
  date.setHours(hour, 0, 0, 0);
  return release({ published_at: date.toISOString(), ...overrides });
}

function populated(releases: Release[], overrides: Partial<ReleasesResponse> = {}) {
  state = { ...IDLE, listing: { ...listing(releases), sources: SOURCES, ...overrides } };
}

function renderPage() {
  const ReleasesPage = page.default;
  return render(
    <MemoryRouter initialEntries={['/releases']}>
      <ReleasesPage />
    </MemoryRouter>
  );
}

describe('ReleasesPage', () => {
  beforeEach(() => {
    mockRefresh.mockReset();
    state = IDLE;
    permissions = ['releases:read', 'releases:create'];
    writable = true;
  });

  it('titles the page and shows a skeleton while loading', () => {
    state = { ...IDLE, loading: true };
    renderPage();
    expect(screen.getByRole('heading', { level: 1, name: 'Releases' })).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('Loading releases');
  });

  it('groups releases into days, newest first', () => {
    populated([
      onDay(0, 9, { id: 1, tag: 'v1.0.1' }),
      onDay(2, 9, { id: 2, tag: 'v1.0.0' }),
      onDay(0, 10, { id: 3, tag: 'v1.0.2' }),
    ]);
    renderPage();

    const days = screen.getAllByRole('region');
    expect(days).toHaveLength(2);
    expect(
      within(days[0])
        .getAllByRole('article')
        .map((card) => card.getAttribute('aria-label'))
    ).toEqual(['acme/zone v1.0.2', 'acme/zone v1.0.1']);
    expect(within(days[1]).getAllByRole('article')).toHaveLength(1);
  });

  it('filters by repository and offers to clear an empty filter', () => {
    populated([recent(1, { id: 1, tag: 'v1.0.1' })]);
    renderPage();

    chooseSelect('Filter by repository', 'acme/console');
    expect(screen.getByText(page.NO_MATCHES_TITLE)).toBeInTheDocument();
    expect(screen.queryByRole('article')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: page.SHOW_ALL }));
    expect(screen.getAllByRole('article')).toHaveLength(1);
  });

  it('links to the sources settings when no source is connected', () => {
    state = { ...IDLE, listing: listing([]) };
    renderPage();
    expect(screen.getByText(NO_SOURCES_TITLE)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: CONNECT_SOURCE })).toHaveAttribute(
      'href',
      '/settings/workspace/sources'
    );
  });

  it('says nothing was published inside the window', () => {
    populated([]);
    renderPage();
    expect(screen.getByText(page.noReleasesDescription(7))).toBeInTheDocument();
  });

  it('warns that a stale or partial history may be incomplete', () => {
    populated([recent(1)], { stale: true, partial: true, warnings: ['acme/console: 403'] });
    renderPage();
    const notices = within(screen.getByRole('list', { name: NOTICES_LABEL }));
    expect(notices.getByText(page.STALE_NOTICE)).toBeInTheDocument();
    expect(notices.getByText(page.PARTIAL_NOTICE)).toBeInTheDocument();
    expect(notices.getByText('acme/console: 403')).toBeInTheDocument();
  });

  it('names the repository whose source could not be read', () => {
    populated([recent(1)], {
      partial: true,
      sources: [SOURCES[0], { ...SOURCES[1], freshness: 'stale', warning: 'token expired' }],
    });
    renderPage();
    const notices = within(screen.getByRole('list', { name: NOTICES_LABEL }));
    expect(notices.getByText('acme/console: token expired')).toBeInTheDocument();
  });

  it('reports a load failure', () => {
    state = { ...IDLE, error: 'Failed to load releases' };
    renderPage();
    expect(screen.getByRole('alert')).toHaveTextContent('Failed to load releases');
  });

  it('says GitHub is unreachable as a status instead of an error', () => {
    state = { ...IDLE, error: 'Bad gateway', unavailable: true };
    renderPage();
    expect(screen.getByRole('status')).toHaveTextContent(UNAVAILABLE);
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('refreshes past the server cache, once at a time', () => {
    populated([recent(1)]);
    renderPage();
    fireEvent.click(screen.getByRole('button', { name: page.REFRESH_LABEL }));
    expect(mockRefresh).toHaveBeenCalledTimes(1);
  });

  it('disables refresh while a refresh runs', () => {
    populated([recent(1)]);
    state = { ...state, refreshing: true };
    renderPage();
    expect(screen.getByRole('button', { name: page.REFRESH_LABEL })).toBeDisabled();
  });

  it('opens the release wizard', async () => {
    populated([recent(1)]);
    renderPage();
    fireEvent.click(screen.getByRole('button', { name: page.NEW_RELEASE }));
    await waitFor(() =>
      expect(screen.getByRole('dialog', { name: 'New release' })).toBeInTheDocument()
    );
  });

  it('offers no new release without the create permission', () => {
    permissions = ['releases:read'];
    populated([recent(1)]);
    renderPage();
    expect(screen.queryByRole('button', { name: page.NEW_RELEASE })).toBeNull();
  });

  it('offers no new release in a read-only workspace', () => {
    writable = false;
    populated([recent(1)]);
    renderPage();
    expect(screen.queryByRole('button', { name: page.NEW_RELEASE })).toBeNull();
  });
});
