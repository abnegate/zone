import { afterAll, beforeAll, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import { ApiError } from '../../../api/ApiError';
import { HttpStatus } from '../../../api/HttpStatus';
import {
  HEAD,
  wireCommits,
  wireDetail,
  wireDiff,
  wireLog,
  wirePullRequest,
} from '../../../test/pullRequests';
import {
  CheckLogSchema,
  CommitListSchema,
  DiffSchema,
  PullRequestDetailSchema,
  PullRequestSchema,
} from '../schemas';
import type { PullRequest } from '../types';
import { MORE_THREADS, NOTHING_BLOCKING } from './PullRequestBlockers';

const mockDetail = mock();
const mockDiff = mock();
const mockCommits = mock();
const mockCommitDiff = mock();
const mockJobLog = mock();

mock.module('../../../api/pullRequests', () => ({
  pullRequestsApi: {
    detail: mockDetail,
    diff: mockDiff,
    commits: mockCommits,
    commitDiff: mockCommitDiff,
    jobLog: mockJobLog,
    merge: mock(),
  },
}));

let writable = true;

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: { id: 'workspace-1' }, loading: false, writable }),
}));

let granted: string[] = ['pull_requests:read', 'pull_requests:update'];

mock.module('../../auth', () => ({
  useAuth: () => ({
    hasPermission: (permission: string) => granted.includes(permission),
    hasAnyPermission: (permissions: string[]) => permissions.some((each) => granted.includes(each)),
    hasAllPermissions: (permissions: string[]) =>
      permissions.every((each) => granted.includes(each)),
  }),
}));

let PullRequestDetail: typeof import('./PullRequestDetail').PullRequestDetail;

beforeAll(async () => {
  ({ PullRequestDetail } = await import('./PullRequestDetail'));
});

afterAll(() => {
  mock.restore();
});

const notReady = PullRequestSchema.parse(wirePullRequest);

const ready: PullRequest = {
  ...notReady,
  number: 7,
  readiness: 'ready',
  blockers: [],
  check_details: [],
  checks: { ...notReady.checks, state: 'success' },
};

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

function renderDetail(pullRequest: PullRequest = notReady, onClose = mock()) {
  render(<PullRequestDetail pullRequest={pullRequest} onClose={onClose} />, { wrapper });
  return onClose;
}

describe('PullRequestDetail', () => {
  beforeEach(() => {
    for (const each of [mockDetail, mockDiff, mockCommits, mockCommitDiff, mockJobLog]) {
      each.mockReset();
    }
    mockDetail.mockResolvedValue(PullRequestDetailSchema.parse(wireDetail));
    mockDiff.mockResolvedValue(DiffSchema.parse(wireDiff));
    mockCommits.mockResolvedValue(CommitListSchema.parse(wireCommits));
    mockCommitDiff.mockResolvedValue(DiffSchema.parse(wireDiff));
    mockJobLog.mockResolvedValue(CheckLogSchema.parse(wireLog));
    writable = true;
    granted = ['pull_requests:read', 'pull_requests:update'];
  });

  it('lays out the facts and the blockers of the pull request', async () => {
    renderDetail();

    expect(screen.getByRole('heading', { level: 2 })).toHaveTextContent(notReady.title);
    expect(screen.getByRole('link', { name: 'acme/zone #42' })).toHaveAttribute(
      'href',
      notReady.url
    );
    expect(screen.getByText('feat/pull-requests')).toBeInTheDocument();
    expect(screen.getByText(HEAD.slice(0, 7))).toBeInTheDocument();
    expect(within(screen.getByRole('list', { name: 'Blockers' })).getByText('1 check failed'));
    await screen.findByText('Handle the error here.');
  });

  it('reads the detail at once but the diff and commits only when their tab opens', async () => {
    const user = userEvent.setup();
    renderDetail();

    await screen.findByText('Handle the error here.');
    expect(mockDetail).toHaveBeenCalledWith('workspace-1', notReady, expect.any(AbortSignal));
    expect(mockDiff).not.toHaveBeenCalled();
    expect(mockCommits).not.toHaveBeenCalled();

    await user.click(screen.getByRole('tab', { name: 'Diff' }));
    await screen.findByText('@@ -1,2 +1,2 @@');
    expect(mockDiff).toHaveBeenCalledWith('workspace-1', notReady, HEAD, expect.any(AbortSignal));
    expect(mockCommits).not.toHaveBeenCalled();

    await user.click(screen.getByRole('tab', { name: 'Commits' }));
    await screen.findByText('feat(console): add the pull requests page');
    expect(mockCommits).toHaveBeenCalledTimes(1);
  });

  it('loads the diff of a commit once it is selected', async () => {
    const user = userEvent.setup();
    renderDetail();

    await user.click(screen.getByRole('tab', { name: 'Commits' }));
    const commit = await screen.findByRole('button', { name: /add the pull requests page/ });
    expect(mockCommitDiff).not.toHaveBeenCalled();

    await user.click(commit);
    await screen.findByText('@@ -1,2 +1,2 @@');
    expect(commit).toHaveAttribute('aria-pressed', 'true');
    expect(mockCommitDiff).toHaveBeenCalledWith(
      'workspace-1',
      notReady,
      HEAD,
      HEAD,
      expect.any(AbortSignal)
    );
  });

  it('reads a failed check log only when asked to show it', async () => {
    const user = userEvent.setup();
    renderDetail();

    const failed = screen.getByRole('region', { name: 'Failed checks' });
    expect(within(failed).getByText('test')).toBeInTheDocument();
    expect(mockJobLog).not.toHaveBeenCalled();

    await user.click(within(failed).getByRole('button', { name: 'Show logs' }));
    await screen.findByText('error: test failed');
    expect(mockJobLog).toHaveBeenCalledWith(
      'workspace-1',
      notReady,
      9001,
      HEAD,
      expect.any(AbortSignal)
    );
    expect(within(failed).getByRole('button', { name: 'Hide logs' })).toBeInTheDocument();
  });

  it('offers no log for a failed check that is not an Actions job', () => {
    renderDetail({
      ...notReady,
      check_details: notReady.check_details.map((check) => ({
        ...check,
        id: null,
        url: 'https://ci.example.com/build/7',
      })),
    });

    const failed = screen.getByRole('region', { name: 'Failed checks' });
    expect(within(failed).getByText('test')).toBeInTheDocument();
    expect(within(failed).queryByRole('button', { name: 'Show logs' })).toBeNull();
    expect(within(failed).getByRole('link', { name: 'Open test on GitHub' })).toHaveAttribute(
      'href',
      'https://ci.example.com/build/7'
    );
  });

  it('lists unresolved threads and bot reviews', async () => {
    renderDetail();

    const threads = await screen.findByRole('region', { name: 'Unresolved threads' });
    expect(within(threads).getByText('src/main.rs:12')).toBeInTheDocument();
    expect(within(threads).queryByRole('link', { name: MORE_THREADS })).toBeNull();
    const reviews = screen.getByRole('region', { name: 'Bot reviews' });
    expect(within(reviews).getByText('Confidence score: 5/5')).toBeInTheDocument();
  });

  it('points to GitHub when the detail read only some of the threads', async () => {
    const detail = PullRequestDetailSchema.parse(wireDetail);
    mockDetail.mockResolvedValue({
      ...detail,
      pull_request: { ...detail.pull_request, threads_complete: false },
    });
    renderDetail();

    const threads = await screen.findByRole('region', { name: 'Unresolved threads' });
    expect(within(threads).getByText('src/main.rs:12')).toBeInTheDocument();
    expect(within(threads).getByRole('link', { name: MORE_THREADS })).toHaveAttribute(
      'href',
      notReady.url
    );
  });

  it('points to GitHub when the detail read no thread but there are more', async () => {
    const detail = PullRequestDetailSchema.parse(wireDetail);
    mockDetail.mockResolvedValue({
      ...detail,
      threads: [],
      comments: [],
      pull_request: { ...detail.pull_request, threads_complete: false },
    });
    renderDetail(ready);

    const threads = await screen.findByRole('region', { name: 'Unresolved threads' });
    expect(within(threads).getByRole('link', { name: MORE_THREADS })).toBeInTheDocument();
    expect(screen.queryByText(NOTHING_BLOCKING)).toBeNull();
  });

  it('judges the threads by the detail read, not the listing', async () => {
    renderDetail({ ...notReady, threads_complete: false });

    const threads = await screen.findByRole('region', { name: 'Unresolved threads' });
    expect(within(threads).queryByRole('link', { name: MORE_THREADS })).toBeNull();
  });

  it('shows why the detail could not load', async () => {
    mockDetail.mockRejectedValue(new ApiError('Pull request not found', HttpStatus.NotFound));
    renderDetail();

    expect(await screen.findByRole('alert')).toHaveTextContent('Pull request not found');
  });

  it('offers Merge only on a ready pull request', async () => {
    const { rerender } = render(<PullRequestDetail pullRequest={notReady} onClose={mock()} />, {
      wrapper,
    });
    expect(screen.getByRole('button', { name: 'Merge' })).toBeDisabled();

    rerender(<PullRequestDetail pullRequest={ready} onClose={mock()} />);
    expect(screen.getByRole('button', { name: 'Merge' })).toBeEnabled();
  });

  it('never offers Merge on a draft or a pull request without a head', () => {
    const { rerender } = render(
      <PullRequestDetail pullRequest={{ ...ready, draft: true }} onClose={mock()} />,
      { wrapper }
    );
    expect(screen.getByRole('button', { name: 'Merge' })).toBeDisabled();

    rerender(<PullRequestDetail pullRequest={{ ...ready, head: null }} onClose={mock()} />);
    expect(screen.getByRole('button', { name: 'Merge' })).toBeDisabled();
  });

  it('hides Merge without the update permission', () => {
    granted = ['pull_requests:read'];
    renderDetail(ready);

    expect(screen.queryByRole('button', { name: 'Merge' })).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Open on GitHub' })).toHaveAttribute('href', ready.url);
  });

  it('hides Merge in a workspace the user can only read', () => {
    writable = false;
    renderDetail(ready);

    expect(screen.queryByRole('button', { name: 'Merge' })).not.toBeInTheDocument();
  });

  it('opens the merge dialog', async () => {
    const user = userEvent.setup();
    renderDetail(ready);

    await user.click(screen.getByRole('button', { name: 'Merge' }));
    expect(await screen.findByRole('dialog', { name: 'Merge pull request' })).toBeInTheDocument();
  });

  it('closes', async () => {
    const user = userEvent.setup();
    const onClose = renderDetail();

    await user.click(screen.getByRole('button', { name: 'Close pull request' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('lists blockers that share a code without clashing keys', () => {
    const errors = spyOn(console, 'error').mockImplementation(() => {});
    renderDetail({
      ...notReady,
      blockers: [
        { code: 'checks_failed', detail: 'lint failed' },
        { code: 'checks_failed', detail: 'test failed' },
      ],
    });

    expect(
      within(screen.getByRole('list', { name: 'Blockers' })).getAllByRole('listitem')
    ).toHaveLength(2);
    expect(errors.mock.calls.flat().join(' ')).not.toContain('same key');
    errors.mockRestore();
  });

  it('waits for the detail before saying nothing blocks', async () => {
    mockDetail.mockResolvedValue({
      ...PullRequestDetailSchema.parse(wireDetail),
      threads: [],
      comments: [],
    });
    renderDetail(ready);

    expect(screen.getByRole('status')).toHaveTextContent('Loading review threads');
    await waitFor(() =>
      expect(
        screen.getByText('No failed checks, open threads or bot reviews to show')
      ).toBeInTheDocument()
    );
  });
});
