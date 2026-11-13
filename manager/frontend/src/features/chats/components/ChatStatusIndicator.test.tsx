import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import type { ChatIndicator, ChatPullIndicator, PullRequestStatus } from '../types';
import { ChatStatusIndicator } from './ChatStatusIndicator';

const STATUS: PullRequestStatus = {
  title: 'Add widgets',
  state: 'open',
  draft: false,
  additions: 1,
  deletions: 1,
  changed_files: 1,
  checks: 'passing',
  mergeable: 'clean',
  auto_merge: { enabled: false, unavailable: null },
  threads: { open: 0, resolved: 0 },
  draft_unavailable: null,
  observed_at: '2026-11-13T10:00:00Z',
};

function pull(status: Partial<PullRequestStatus> | null, ready = false): ChatPullIndicator {
  return {
    id: 'pr-1',
    number: 12,
    url: 'https://github.com/acme/widgets/pull/12',
    title: 'Add widgets',
    status: status === null ? null : { ...STATUS, ...status },
    ready,
  };
}

function indicator(overrides: Partial<ChatIndicator> = {}): ChatIndicator {
  return {
    chat_id: 'chat-1',
    activity: 'idle',
    pull_request: null,
    pull_requests: 0,
    ...overrides,
  };
}

describe('ChatStatusIndicator', () => {
  it('shows nothing for an idle chat with no pull request', () => {
    render(<ChatStatusIndicator indicator={indicator()} />);

    expect(screen.queryByTestId('chat-status')).toBeNull();
  });

  it('shows nothing before any reading has arrived', () => {
    render(<ChatStatusIndicator />);

    expect(screen.queryByTestId('chat-status')).toBeNull();
  });

  it('marks a chat that is still writing', () => {
    render(<ChatStatusIndicator indicator={indicator({ activity: 'writing' })} />);

    expect(screen.getByLabelText('Writing a reply')).toBeInTheDocument();
  });

  it('says which kind of answer a stopped chat is waiting for', () => {
    render(<ChatStatusIndicator indicator={indicator({ activity: 'awaiting_approval' })} />);
    expect(screen.getByLabelText('Waiting for you to approve a tool call')).toBeInTheDocument();

    render(<ChatStatusIndicator indicator={indicator({ activity: 'awaiting_answer' })} />);
    expect(screen.getByLabelText('Waiting for your answer')).toBeInTheDocument();
  });

  it('shows both the live turn and the failing pull request, rather than one of them', () => {
    render(
      <ChatStatusIndicator
        indicator={indicator({
          activity: 'writing',
          pull_request: pull({ checks: 'failing' }),
          pull_requests: 1,
        })}
      />
    );

    expect(screen.getByTestId('chat-status-writing')).toBeInTheDocument();
    expect(screen.getByTestId('chat-status-pull-failing')).toBeInTheDocument();
    expect(screen.getByTestId('chat-status')).toHaveAttribute(
      'title',
      'Writing a reply · #12: Checks failing'
    );
  });

  it('marks a pull request that can merge as it stands', () => {
    render(
      <ChatStatusIndicator
        indicator={indicator({ pull_request: pull({}, true), pull_requests: 1 })}
      />
    );

    expect(screen.getByLabelText('Pull request #12: Ready to merge')).toBeInTheDocument();
  });

  it('says a status could not be read rather than drawing it as passing', () => {
    render(
      <ChatStatusIndicator indicator={indicator({ pull_request: pull(null), pull_requests: 1 })} />
    );

    expect(screen.getByTestId('chat-status-pull-unread')).toBeInTheDocument();
    expect(
      screen.getByLabelText('Pull request #12: Pull request status unavailable')
    ).toBeInTheDocument();
  });

  it('names the unresolved review comments holding an otherwise green pull request back', () => {
    render(
      <ChatStatusIndicator
        indicator={indicator({
          pull_request: pull({ threads: { open: 2, resolved: 0 } }, false),
          pull_requests: 1,
        })}
      />
    );

    expect(
      screen.getByLabelText('Pull request #12: 2 unresolved review comments')
    ).toBeInTheDocument();
  });
});
