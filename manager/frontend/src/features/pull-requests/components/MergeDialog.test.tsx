import { afterAll, beforeAll, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import { ApiError } from '../../../api/ApiError';
import { HttpStatus } from '../../../api/HttpStatus';
import { HEAD, wireMerged, wirePullRequest } from '../../../test/pullRequests';
import { chooseSelect } from '../../../test/select';
import { PullRequestRefusal } from '../PullRequestRefusal';
import { MergedSchema, PullRequestSchema } from '../schemas';

const mockMerge = mock();
const mockToast = mock();

mock.module('../../../api/pullRequests', () => ({
  pullRequestsApi: { merge: mockMerge },
}));

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: { id: 'workspace-1' }, loading: false, writable: true }),
}));

mock.module('sonner', () => ({ toast: { success: mockToast } }));

let MergeDialog: typeof import('./MergeDialog').MergeDialog;
let HEAD_MOVED: string;
let OPEN_ON_GITHUB: string;
let MERGE_OUTCOME_UNKNOWN: string;

beforeAll(async () => {
  ({ MergeDialog, HEAD_MOVED, OPEN_ON_GITHUB } = await import('./MergeDialog'));
  ({ MERGE_OUTCOME_UNKNOWN } = await import('../hooks/useMergePullRequest'));
});

const MOVED = 'd'.repeat(40);

afterAll(() => {
  mock.restore();
});

const pullRequest = { ...PullRequestSchema.parse(wirePullRequest), readiness: 'ready' as const };

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

function renderDialog(onClose = mock()) {
  render(<MergeDialog isOpen pullRequest={pullRequest} onClose={onClose} />, { wrapper });
  return onClose;
}

describe('MergeDialog', () => {
  beforeEach(() => {
    mockMerge.mockReset();
    mockToast.mockReset();
  });

  it('describes the merge it is about to make', () => {
    renderDialog();

    const dialog = screen.getByRole('dialog', { name: 'Merge pull request' });
    expect(dialog).toHaveTextContent('Merge #42 Add the pull requests page');
    expect(dialog).toHaveTextContent('feat/pull-requests');
    expect(dialog).toHaveTextContent(HEAD.slice(0, 7));
  });

  it('merges the expected head with the chosen method, then closes', async () => {
    const user = userEvent.setup();
    mockMerge.mockResolvedValue(MergedSchema.parse(wireMerged));
    const onClose = renderDialog();

    chooseSelect('Merge method', 'Squash and merge');
    await user.click(screen.getByRole('button', { name: 'Merge' }));

    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(mockMerge).toHaveBeenCalledWith('workspace-1', pullRequest, {
      expected_head: HEAD,
      method: 'squash',
    });
    expect(mockToast).toHaveBeenCalledWith('Merged acme/zone #42');
  });

  it('shows a refusal and its blockers and stays open', async () => {
    const user = userEvent.setup();
    mockMerge.mockRejectedValue(
      new PullRequestRefusal({
        error: 'The pull request is no longer ready',
        code: 'not_ready',
        blockers: [{ code: 'checks_failed', detail: '1 check failed' }],
      })
    );
    const onClose = renderDialog();

    await user.click(screen.getByRole('button', { name: 'Merge' }));

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('The pull request is no longer ready');
    expect(within(alert).getByText('1 check failed')).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
    expect(mockToast).not.toHaveBeenCalled();
  });

  it.each([
    ['merged', 'The pull request was already merged.'],
    ['closed', 'The pull request is closed. Reopen it to merge it.'],
  ] as const)('says the pull request was %s and stops offering it', async (code, message) => {
    const user = userEvent.setup();
    mockMerge.mockRejectedValue(new PullRequestRefusal({ error: message, code }));
    renderDialog();

    await user.click(screen.getByRole('button', { name: 'Merge' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(message);
    expect(screen.getByRole('button', { name: 'Merge' })).toBeDisabled();
    expect(screen.queryByRole('link', { name: OPEN_ON_GITHUB })).toBeNull();
  });

  it('asks for GitHub to be checked when the merge stopped without an answer', async () => {
    const user = userEvent.setup();
    mockMerge.mockRejectedValue(
      new ApiError(
        'Failed to merge pull request: The merge stopped before it reported back',
        HttpStatus.InternalServerError
      )
    );
    const onClose = renderDialog();

    await user.click(screen.getByRole('button', { name: 'Merge' }));

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent(MERGE_OUTCOME_UNKNOWN);
    expect(within(alert).getByRole('link', { name: OPEN_ON_GITHUB })).toHaveAttribute(
      'href',
      pullRequest.url
    );
    expect(screen.getByRole('button', { name: 'Merge' })).toBeDisabled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it('shows a missing permission and lets the merge be tried again', async () => {
    const user = userEvent.setup();
    const message = 'Failed to merge pull request: Missing permission: pull_requests:update';
    mockMerge.mockRejectedValue(new ApiError(message, HttpStatus.Forbidden));
    renderDialog();

    await user.click(screen.getByRole('button', { name: 'Merge' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(message);
    expect(screen.getByRole('button', { name: 'Merge' })).toBeEnabled();
  });

  it('holds the merge once new commits reach the list while the dialog is open', async () => {
    const { rerender } = render(<MergeDialog isOpen pullRequest={pullRequest} onClose={mock()} />, {
      wrapper,
    });

    rerender(<MergeDialog isOpen pullRequest={{ ...pullRequest, head: MOVED }} onClose={mock()} />);

    expect(screen.getByRole('status')).toHaveTextContent(HEAD_MOVED);
    expect(screen.getByRole('dialog')).toHaveTextContent(HEAD.slice(0, 7));
    const merge = screen.getByRole('button', { name: 'Merge' });
    expect(merge).toBeDisabled();
    fireEvent.click(merge);
    expect(mockMerge).not.toHaveBeenCalled();
  });

  it('merges the head it showed even when the list moves on mid-merge', async () => {
    let finish: (value: unknown) => void = () => {};
    mockMerge.mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    const onClose = mock();
    const { rerender } = render(
      <MergeDialog isOpen pullRequest={pullRequest} onClose={onClose} />,
      { wrapper }
    );

    fireEvent.click(screen.getByRole('button', { name: 'Merge' }));
    rerender(
      <MergeDialog isOpen pullRequest={{ ...pullRequest, head: MOVED }} onClose={onClose} />
    );
    await act(async () => finish(MergedSchema.parse(wireMerged)));

    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(mockMerge).toHaveBeenCalledTimes(1);
    expect(mockMerge.mock.calls[0][1]).toMatchObject({ head: HEAD });
    expect(mockMerge.mock.calls[0][2]).toMatchObject({ expected_head: HEAD });
  });

  it('holds the merge after GitHub says the head moved', async () => {
    const user = userEvent.setup();
    mockMerge.mockRejectedValue(
      new PullRequestRefusal({
        error: 'The pull request head moved',
        code: 'head_moved',
        current_head: MOVED,
      })
    );
    renderDialog();

    await user.click(screen.getByRole('button', { name: 'Merge' }));

    expect(await screen.findByRole('status')).toHaveTextContent(HEAD_MOVED);
    expect(screen.getByRole('button', { name: 'Merge' })).toBeDisabled();
  });

  it('sends one merge however fast Merge is pressed', async () => {
    mockMerge.mockReturnValue(new Promise(() => {}));
    renderDialog();
    const merge = screen.getByRole('button', { name: 'Merge' });

    act(() => {
      fireEvent.click(merge);
      fireEvent.click(merge);
    });

    await waitFor(() => expect(mockMerge).toHaveBeenCalledTimes(1));
  });

  it('tells the page once a merge lands', async () => {
    const user = userEvent.setup();
    mockMerge.mockResolvedValue(MergedSchema.parse(wireMerged));
    const onMerged = mock();
    render(<MergeDialog isOpen pullRequest={pullRequest} onClose={mock()} onMerged={onMerged} />, {
      wrapper,
    });

    await user.click(screen.getByRole('button', { name: 'Merge' }));

    await waitFor(() => expect(onMerged).toHaveBeenCalledTimes(1));
  });

  it('lists blockers that share a code without clashing keys', async () => {
    const user = userEvent.setup();
    const errors = spyOn(console, 'error').mockImplementation(() => {});
    mockMerge.mockRejectedValue(
      new PullRequestRefusal({
        error: 'The pull request is no longer ready',
        code: 'not_ready',
        blockers: [
          { code: 'checks_failed', detail: 'lint failed' },
          { code: 'checks_failed', detail: 'test failed' },
        ],
      })
    );
    renderDialog();

    await user.click(screen.getByRole('button', { name: 'Merge' }));

    const alert = await screen.findByRole('alert');
    expect(within(alert).getAllByRole('listitem')).toHaveLength(2);
    expect(errors.mock.calls.flat().join(' ')).not.toContain('same key');
    errors.mockRestore();
  });

  it('cancels without merging', async () => {
    const user = userEvent.setup();
    const onClose = renderDialog();

    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(mockMerge).not.toHaveBeenCalled();
  });
});
