import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import { Link, MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { ApiError } from '../../../api/ApiError';
import { HttpStatus } from '../../../api/HttpStatus';
import { SOURCE_ID, wireDetail, wirePullRequests } from '../../../test/pullRequests';
import { chooseSelect } from '../../../test/select';
import { UNAVAILABLE } from '../../github';
import { PullRequestDetailSchema, PullRequestsResponseSchema } from '../schemas';
import type { PullRequest, PullRequestsResponse } from '../types';

const mockList = mock();
const mockDetail = mock();
const mockMerge = mock();

mock.module('../../../api/pullRequests', () => ({
  pullRequestsApi: { list: mockList, detail: mockDetail, merge: mockMerge },
}));

mock.module('sonner', () => ({ toast: { success: mock() } }));

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: { id: 'workspace-1' }, loading: false, writable: true }),
}));

mock.module('../../auth', () => ({
  useAuth: () => ({
    hasPermission: () => true,
    hasAnyPermission: () => true,
    hasAllPermissions: () => true,
  }),
}));

let PullRequestsPage: typeof import('./PullRequestsPage').default;
let page: typeof import('./PullRequestsPage');

beforeAll(async () => {
  page = await import('./PullRequestsPage');
  PullRequestsPage = page.default;
});

afterAll(() => {
  mock.restore();
});

const LONG_AGO = '2026-01-01T00:00:00Z';
const OTHER_SOURCE = '0f0e0d0c-0b0a-4908-8706-050403020100';

const base = PullRequestsResponseSchema.parse(wirePullRequests);
const [first] = base.pull_requests;

function pullRequest(changes: Partial<PullRequest>): PullRequest {
  return { ...first, ...changes };
}

const ready = pullRequest({
  number: 1,
  title: 'Ready change',
  readiness: 'ready',
  author: 'hubot',
});
const running = pullRequest({
  number: 2,
  title: 'Running change',
  readiness: 'in_progress',
  repository: 'acme/api',
  source_id: OTHER_SOURCE,
});
const blocked = pullRequest({ number: 42, title: 'Blocked change' });

function listing(changes: Partial<PullRequestsResponse> = {}): PullRequestsResponse {
  return {
    ...base,
    generated_at: LONG_AGO,
    pull_requests: [ready, running, blocked],
    ...changes,
  };
}

function Location() {
  const location = useLocation();
  return <output data-testid="location">{`${location.pathname}${location.search}`}</output>;
}

function renderPage(path = '/pull-requests') {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <MemoryRouter initialEntries={[path]}>
        <QueryClientProvider client={client}>
          <Routes>
            <Route
              path="/pull-requests"
              element={
                <>
                  <Location />
                  <Link to={`/pull-requests?id=${SOURCE_ID}:42`}>Follow the link</Link>
                  {children}
                </>
              }
            />
            <Route path="*" element={<Location />} />
          </Routes>
        </QueryClientProvider>
      </MemoryRouter>
    );
  }
  return render(<PullRequestsPage />, { wrapper: Wrapper });
}

function cards(): string[] {
  return screen
    .queryAllByTestId('pull-request-card')
    .map((card) => card.querySelector('h3')?.textContent ?? '');
}

describe('PullRequestsPage', () => {
  beforeEach(() => {
    mockList.mockReset();
    mockDetail.mockReset();
    mockMerge.mockReset();
    mockList.mockResolvedValue(listing());
    mockDetail.mockResolvedValue(PullRequestDetailSchema.parse(wireDetail));
  });

  it('shows a skeleton under the page bar while the list loads', () => {
    mockList.mockReturnValue(new Promise(() => {}));
    renderPage();

    expect(screen.getByRole('heading', { name: 'Pull Requests', level: 1 })).toBeInTheDocument();
    expect(screen.getByText('Loading pull requests')).toHaveAttribute('role', 'status');
    expect(cards()).toEqual([]);
  });

  it('groups the open pull requests by readiness and asks to pick one', async () => {
    renderPage();

    await waitFor(() =>
      expect(cards()).toEqual(['Ready change', 'Running change', 'Blocked change'])
    );
    expect(mockList).toHaveBeenCalledWith('workspace-1', false, expect.any(AbortSignal));
    expect(screen.getByText('Select a pull request')).toBeInTheDocument();
  });

  it('links to workspace sources when no GitHub source is connected', async () => {
    mockList.mockResolvedValue(listing({ sources: [], pull_requests: [] }));
    renderPage();

    expect(await screen.findByText('No GitHub sources')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Connect a source' })).toHaveAttribute(
      'href',
      '/settings/workspace/sources'
    );
  });

  it('says when the sources have no open pull requests', async () => {
    mockList.mockResolvedValue(listing({ pull_requests: [] }));
    renderPage();

    expect(await screen.findByText('No open pull requests')).toBeInTheDocument();
  });

  it('shows why the list could not load', async () => {
    mockList.mockRejectedValue(new ApiError('Workspace not found', HttpStatus.NotFound));
    renderPage();

    expect(await screen.findByRole('alert')).toHaveTextContent('Workspace not found');
  });

  it('says GitHub is unavailable while the gateway fails', async () => {
    mockList.mockRejectedValue(new ApiError('Bad gateway', HttpStatus.BadGateway));
    renderPage();

    expect(await screen.findByText('GitHub is unavailable')).toBeInTheDocument();
    expect(screen.getByText(UNAVAILABLE)).toBeInTheDocument();
  });

  it('filters by readiness from a radio group', async () => {
    const user = userEvent.setup();
    renderPage();
    await waitFor(() => expect(cards()).toHaveLength(3));
    const group = screen.getByRole('radiogroup', { name: 'Filter by readiness' });
    expect(screen.queryByRole('tablist')).not.toBeInTheDocument();

    await user.click(within(group).getByRole('radio', { name: 'Ready' }));
    expect(cards()).toEqual(['Ready change']);
    expect(within(group).getByRole('radio', { name: 'Ready' })).toHaveAttribute(
      'aria-checked',
      'true'
    );

    await user.click(within(group).getByRole('radio', { name: 'Not ready' }));
    expect(cards()).toEqual(['Blocked change']);

    await user.click(within(group).getByRole('radio', { name: 'All' }));
    expect(cards()).toHaveLength(3);
  });

  it('moves the readiness choice with the arrow keys and keeps one stop in the tab order', async () => {
    const user = userEvent.setup();
    renderPage();
    await waitFor(() => expect(cards()).toHaveLength(3));
    const radios = within(screen.getByRole('radiogroup')).getAllByRole('radio');
    expect(radios.map((radio) => radio.tabIndex)).toEqual([0, -1, -1, -1]);

    radios[0].focus();
    await user.keyboard('{ArrowRight}');

    expect(document.activeElement).toBe(radios[1]);
    expect(radios[1]).toHaveAttribute('aria-checked', 'true');
    expect(cards()).toEqual(['Ready change']);

    await user.keyboard('{ArrowLeft}{ArrowLeft}');
    expect(radios[3]).toHaveAttribute('aria-checked', 'true');
  });

  it('filters by repository and author, and clears filters that match nothing', async () => {
    const user = userEvent.setup();
    renderPage();
    await waitFor(() => expect(cards()).toHaveLength(3));

    chooseSelect('Filter by repository', 'acme/api');
    expect(cards()).toEqual(['Running change']);

    chooseSelect('Filter by author', 'hubot');
    expect(cards()).toEqual([]);
    expect(screen.getByText('No matching pull requests')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Clear filters' }));
    expect(cards()).toHaveLength(3);
  });

  it('opens the deep-linked pull request and clears the link on close', async () => {
    const user = userEvent.setup();
    renderPage(`/pull-requests?id=${SOURCE_ID}:42`);

    const detail = await screen.findByRole('complementary', { name: 'Pull request #42' });
    expect(within(detail).getByRole('heading', { level: 2 })).toHaveTextContent('Blocked change');
    expect(screen.getByRole('button', { name: /Blocked change/ })).toHaveAttribute(
      'aria-current',
      'true'
    );

    await user.click(within(detail).getByRole('button', { name: 'Close pull request' }));
    expect(screen.queryByRole('complementary')).not.toBeInTheDocument();
    expect(screen.getByTestId('location')).toHaveTextContent(/^\/pull-requests$/);
  });

  it('follows the same link again after the pull request was closed', async () => {
    const user = userEvent.setup();
    renderPage(`/pull-requests?id=${SOURCE_ID}:42`);
    const detail = await screen.findByRole('complementary', { name: 'Pull request #42' });
    await user.click(within(detail).getByRole('button', { name: 'Close pull request' }));
    expect(screen.queryByRole('complementary')).not.toBeInTheDocument();

    await user.click(screen.getByRole('link', { name: 'Follow the link' }));

    expect(
      await screen.findByRole('complementary', { name: 'Pull request #42' })
    ).toBeInTheDocument();
  });

  it('ignores a deep link to a pull request that is not open and drops it from the address', async () => {
    renderPage(`/pull-requests?id=${SOURCE_ID}:999`);

    await waitFor(() => expect(cards()).toHaveLength(3));
    expect(screen.queryByRole('complementary')).not.toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByTestId('location')).toHaveTextContent(/^\/pull-requests$/)
    );
  });

  it('opens a pull request from its card and links to it', async () => {
    const user = userEvent.setup();
    renderPage();

    await user.click(await screen.findByRole('button', { name: /Running change/ }));
    expect(screen.getByRole('complementary', { name: 'Pull request #2' })).toBeInTheDocument();
    expect(screen.getByTestId('location')).toHaveTextContent(
      `/pull-requests?id=${encodeURIComponent(`${OTHER_SOURCE}:2`)}`
    );

    await user.click(screen.getByRole('button', { name: /Ready change/ }));
    expect(screen.getByTestId('location')).toHaveTextContent(
      `/pull-requests?id=${encodeURIComponent(`${SOURCE_ID}:1`)}`
    );
  });

  it('keeps a pull request open while its source could not be read', async () => {
    const user = userEvent.setup();
    renderPage(`/pull-requests?id=${SOURCE_ID}:42`);
    await screen.findByRole('complementary', { name: 'Pull request #42' });

    mockList.mockResolvedValue(
      listing({
        partial: true,
        pull_requests: [running],
        sources: [{ ...base.sources[0], freshness: 'stale', warning: 'GitHub timed out' }],
      })
    );
    await user.click(screen.getByRole('button', { name: 'Refresh pull requests' }));

    await waitFor(() => expect(cards()).toEqual(['Running change']));
    expect(screen.getByRole('complementary', { name: 'Pull request #42' })).toBeInTheDocument();
  });

  it('closes a pull request missing from a partial list when its own source was read', async () => {
    const user = userEvent.setup();
    renderPage(`/pull-requests?id=${SOURCE_ID}:42`);
    await screen.findByRole('complementary', { name: 'Pull request #42' });

    mockList.mockResolvedValue(listing({ partial: true, pull_requests: [running] }));
    await user.click(screen.getByRole('button', { name: 'Refresh pull requests' }));

    await waitFor(() => expect(cards()).toEqual(['Running change']));
    expect(screen.queryByRole('complementary')).not.toBeInTheDocument();
  });

  it('moves focus to the page heading after a merge', async () => {
    const user = userEvent.setup();
    mockMerge.mockResolvedValue({ number: 1, sha: null, admin: false, url: ready.url });
    renderPage(`/pull-requests?id=${SOURCE_ID}:1`);
    const detail = await screen.findByRole('complementary', { name: 'Pull request #1' });

    await user.click(within(detail).getByRole('button', { name: 'Merge' }));
    const dialog = await screen.findByRole('dialog', { name: 'Merge pull request' });
    await user.click(within(dialog).getByRole('button', { name: 'Merge' }));

    const heading = screen.getByRole('heading', { name: 'Pull Requests', level: 1 });
    await waitFor(() => expect(document.activeElement).toBe(heading));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('shows stale, partial and per-source warnings over the list', async () => {
    mockList.mockResolvedValue(
      listing({
        stale: true,
        partial: true,
        warnings: ['acme/api: rate limited'],
        sources: [{ ...base.sources[0], warning: 'token expired' }],
      })
    );
    renderPage();

    const notices = await screen.findByRole('list', { name: 'Notices' });
    expect(
      within(notices)
        .getAllByRole('listitem')
        .map((item) => item.textContent)
    ).toEqual([page.STALE, page.PARTIAL, 'acme/api: rate limited', 'acme/zone: token expired']);
  });

  it('refreshes past the floor and closes a pull request that is no longer open', async () => {
    const user = userEvent.setup();
    renderPage(`/pull-requests?id=${SOURCE_ID}:42`);
    await screen.findByRole('complementary', { name: 'Pull request #42' });

    mockList.mockResolvedValue(listing({ pull_requests: [ready, running] }));
    await user.click(screen.getByRole('button', { name: 'Refresh pull requests' }));

    await waitFor(() => expect(cards()).toEqual(['Ready change', 'Running change']));
    expect(mockList).toHaveBeenCalledWith('workspace-1', true);
    expect(screen.queryByRole('complementary')).not.toBeInTheDocument();
    expect(screen.getByTestId('location')).toHaveTextContent(/^\/pull-requests$/);
  });

  it('holds Refresh inside the refresh floor', async () => {
    mockList.mockResolvedValue(listing({ generated_at: new Date().toISOString() }));
    renderPage();
    await waitFor(() => expect(cards()).toHaveLength(3));

    const refresh = screen.getByRole('button', { name: 'Refresh pull requests' });
    expect(refresh).toBeEnabled();
    expect(refresh).toHaveAttribute('aria-disabled', 'true');
    const description = refresh.getAttribute('aria-describedby');
    expect(description && document.getElementById(description)?.textContent).toBe(page.FLOORED);
    expect(refresh).toHaveAttribute('title', page.FLOORED);
    await userEvent.setup().click(refresh);
    expect(mockList).toHaveBeenCalledTimes(1);
  });

  it('says GitHub is unavailable as a status over a list it already read', async () => {
    mockList
      .mockResolvedValueOnce(listing())
      .mockRejectedValue(new ApiError('Bad gateway', HttpStatus.BadGateway));
    renderPage();
    await waitFor(() => expect(cards()).toHaveLength(3));

    await userEvent.setup().click(screen.getByRole('button', { name: 'Refresh pull requests' }));

    await waitFor(() =>
      expect(screen.getAllByRole('status').map((status) => status.textContent)).toContain(
        UNAVAILABLE
      )
    );
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(cards()).toHaveLength(3);
  });
});
