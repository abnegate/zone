import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { chooseMenuItem, openMenu, settleMenu } from '../../../test/menu';
import type { ChatPullRequest, PullRequestStatus } from '../types';

const mockGetChatPullRequests = mock();
const mockUpdateChatPullRequest = mock();
const mockGetChatPullRequestDiff = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: {
    getChatPullRequests: mockGetChatPullRequests,
    updateChatPullRequest: mockUpdateChatPullRequest,
    getChatPullRequestDiff: mockGetChatPullRequestDiff,
  },
}));

let ChangesBox: typeof import('./ChangesBox').ChangesBox;
let useChatPullRequests: typeof import('../hooks/useChatPullRequests').useChatPullRequests;

beforeAll(async () => {
  ({ ChangesBox } = await import('./ChangesBox'));
  ({ useChatPullRequests } = await import('../hooks/useChatPullRequests'));
});

afterAll(() => {
  mock.restore();
});

const STATUS: PullRequestStatus = {
  title: 'Add widgets to the catalogue',
  state: 'open',
  draft: false,
  additions: 40,
  deletions: 3,
  changed_files: 2,
  checks: 'passing',
  mergeable: 'blocked',
  auto_merge: { enabled: false, unavailable: null },
  observed_at: '2026-11-05T10:00:00Z',
};

const OPEN: ChatPullRequest = {
  id: 'pr-1',
  provider: 'github',
  owner: 'acme',
  repo: 'widgets',
  number: 12,
  url: 'https://github.com/acme/widgets/pull/12',
  title: STATUS.title,
  dismissed: false,
  created_at: '2026-11-05T10:00:00Z',
  status: STATUS,
  actionable: true,
};

const MENU = 'Actions for widgets #12';

function withStatus(status: Partial<PullRequestStatus>): ChatPullRequest {
  return { ...OPEN, status: { ...STATUS, ...status } };
}

function Harness() {
  const changes = useChatPullRequests('chat-1', false);
  return <ChangesBox chatId="chat-1" userId="user-1" changes={changes} />;
}

async function shown(pullRequests: ChatPullRequest[]) {
  mockGetChatPullRequests.mockResolvedValue(pullRequests);
  render(<Harness />);
  await screen.findByRole('button', { name: MENU });
}

function menuItem(name: string | RegExp): HTMLElement {
  return screen.getByRole('menuitem', { name });
}

describe('ChangesBox', () => {
  const originalOpen = window.open;

  beforeEach(() => {
    localStorage.clear();
    mockGetChatPullRequests.mockReset();
    mockUpdateChatPullRequest.mockReset();
    mockGetChatPullRequestDiff.mockReset();
  });

  afterEach(() => {
    window.open = originalOpen;
  });

  it('shows each pull request with its status, repository, number and size', async () => {
    await shown([OPEN]);

    const link = screen.getByRole('link', { name: STATUS.title });
    expect(link).toHaveAttribute('href', OPEN.url);
    expect(link).toHaveAttribute('target', '_blank');
    expect(screen.getByText('widgets #12')).toBeInTheDocument();
    expect(screen.getByText(/\+40/)).toBeInTheDocument();
    expect(screen.getByText(/−3/)).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Checks passing' })).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Merging is blocked' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Changes, 1' })).toHaveAttribute(
      'aria-expanded',
      'true'
    );
  });

  it('names every state a pull request can be in', async () => {
    await shown([
      withStatus({ state: 'merged' }),
      { ...withStatus({ draft: true }), id: 'pr-2', number: 13 },
      { ...withStatus({ checks: 'failing' }), id: 'pr-3', number: 14 },
      { ...OPEN, id: 'pr-4', number: 15, status: null },
    ]);

    for (const label of ['Merged', 'Draft', 'Checks failing', 'Status unavailable']) {
      expect(screen.getByRole('img', { name: label })).toBeInTheDocument();
    }
  });

  it('opens it on GitHub and copies its link', async () => {
    const open = mock();
    window.open = open as unknown as typeof window.open;
    const writeText = mock(async () => undefined);
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    });
    await shown([OPEN]);

    await chooseMenuItem(MENU, 'Open on GitHub');
    expect(open).toHaveBeenCalledWith(OPEN.url, '_blank', 'noopener,noreferrer');

    await chooseMenuItem(MENU, 'Copy link');
    expect(writeText).toHaveBeenCalledWith(OPEN.url);
    expect(await screen.findByText('Copied the link to widgets #12.')).toBeInTheDocument();
  });

  it('turns auto-merge on, and off again once it is on', async () => {
    mockUpdateChatPullRequest.mockResolvedValue(
      withStatus({ auto_merge: { enabled: true, unavailable: null } })
    );
    await shown([OPEN]);

    await chooseMenuItem(MENU, 'Merge when ready');
    expect(mockUpdateChatPullRequest).toHaveBeenCalledWith('chat-1', 'pr-1', {
      auto_merge: true,
    });

    await openMenu(MENU);
    fireEvent.click(menuItem('Cancel auto-merge'));
    await settleMenu();
    expect(mockUpdateChatPullRequest).toHaveBeenLastCalledWith('chat-1', 'pr-1', {
      auto_merge: false,
    });
  });

  it('converts it to a draft, and marks a draft ready for review', async () => {
    mockUpdateChatPullRequest.mockResolvedValue(withStatus({ draft: true }));
    await shown([OPEN]);

    await chooseMenuItem(MENU, 'Convert to draft');
    expect(mockUpdateChatPullRequest).toHaveBeenCalledWith('chat-1', 'pr-1', { draft: true });

    await screen.findByRole('img', { name: 'Draft' });
    await chooseMenuItem(MENU, 'Ready for review');
    expect(mockUpdateChatPullRequest).toHaveBeenLastCalledWith('chat-1', 'pr-1', {
      draft: false,
    });
  });

  it('dismisses it from the chat, and brings it back on undo', async () => {
    mockUpdateChatPullRequest.mockImplementation(
      async (_chat: string, _id: string, change: { dismissed?: boolean }) => ({
        ...OPEN,
        dismissed: Boolean(change.dismissed),
      })
    );
    await shown([OPEN]);

    await chooseMenuItem(MENU, 'Dismiss');
    expect(mockUpdateChatPullRequest).toHaveBeenCalledWith('chat-1', 'pr-1', { dismissed: true });
    await waitFor(() => expect(screen.queryByRole('link', { name: STATUS.title })).toBeNull());
    expect(screen.getByText(/Dismissed widgets #12/)).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    });
    expect(mockUpdateChatPullRequest).toHaveBeenLastCalledWith('chat-1', 'pr-1', {
      dismissed: false,
    });
    expect(await screen.findByRole('link', { name: STATUS.title })).toBeInTheDocument();
    expect(screen.queryByText(/Dismissed widgets #12/)).toBeNull();
  });

  it('shows the changes it makes, file by file', async () => {
    mockGetChatPullRequestDiff.mockResolvedValue({
      diff: [
        'diff --git a/src/widgets.rs b/src/widgets.rs',
        'index 1..2 100644',
        '--- a/src/widgets.rs',
        '+++ b/src/widgets.rs',
        '@@ -1,2 +1,2 @@',
        ' use std::fmt;',
        '-pub struct Gadget;',
        '+pub struct Widget;',
        '',
      ].join('\n'),
      truncated: true,
    });
    await shown([OPEN]);

    await chooseMenuItem(MENU, 'View changes');

    const dialog = await screen.findByRole('dialog', { name: 'Changes in widgets #12' });
    expect(mockGetChatPullRequestDiff).toHaveBeenCalledWith('chat-1', 'pr-1');
    expect(await within(dialog).findByText('src/widgets.rs')).toBeInTheDocument();
    expect(within(dialog).getByText('+pub struct Widget;')).toHaveClass('is-added');
    expect(within(dialog).getByText('-pub struct Gadget;')).toHaveClass('is-removed');
    expect(within(dialog).getByText(/too large to show in full/)).toBeInTheDocument();
  });

  it('offers auto-merge disabled, with the reason, when GitHub will not take it', async () => {
    await shown([
      withStatus({
        auto_merge: {
          enabled: false,
          unavailable: 'Auto-merge is turned off for this repository.',
        },
      }),
    ]);

    await openMenu(MENU);
    const merge = menuItem(/Merge when ready/);
    expect(merge).toHaveAttribute('aria-disabled', 'true');
    expect(merge).toHaveTextContent('Auto-merge is turned off for this repository.');
    expect(merge).toHaveAttribute('title', 'Auto-merge is turned off for this repository.');
    fireEvent.click(merge);
    await settleMenu();
    expect(mockUpdateChatPullRequest).not.toHaveBeenCalled();
  });

  it('leaves only open, copy and dismiss to a reader no source lets act', async () => {
    await shown([{ ...OPEN, actionable: false }]);

    await openMenu(MENU);
    for (const name of [/View changes/, /Merge when ready/, /Convert to draft/]) {
      expect(menuItem(name)).toHaveAttribute('aria-disabled', 'true');
    }
    for (const name of ['Open on GitHub', 'Copy link', 'Dismiss']) {
      expect(menuItem(name)).not.toHaveAttribute('aria-disabled');
    }
  });

  it('offers no draft or merge changes once it has merged', async () => {
    await shown([withStatus({ state: 'merged' })]);

    await openMenu(MENU);
    expect(screen.queryByRole('menuitem', { name: /Merge when ready/ })).toBeNull();
    expect(screen.queryByRole('menuitem', { name: /draft/i })).toBeNull();
  });

  it('says so when GitHub refuses a change', async () => {
    mockUpdateChatPullRequest.mockRejectedValue(new Error('It can be merged now.'));
    await shown([OPEN]);

    await chooseMenuItem(MENU, 'Merge when ready');

    expect(await screen.findByRole('alert')).toHaveTextContent('It can be merged now.');
  });

  it('remembers that it was collapsed', async () => {
    await shown([OPEN]);

    fireEvent.click(screen.getByRole('button', { name: 'Changes, 1' }));

    expect(screen.getByRole('button', { name: 'Changes, 1' })).toHaveAttribute(
      'aria-expanded',
      'false'
    );
    expect(localStorage.getItem('zone:chat-panel:user-1:changes')).toBe('true');
  });
});
