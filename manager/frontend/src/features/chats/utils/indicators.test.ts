import { describe, expect, it } from 'bun:test';
import type { ChatIndicator, ChatPullIndicator, PullRequestStatus } from '../types';
import {
  attentionRank,
  byChat,
  CHAT_FILTER_STORAGE_KEY,
  indicatorTitle,
  isChatFilter,
  matchesFilter,
  pullDetail,
  pullSignal,
  readChatFilter,
  writeChatFilter,
} from './indicators';

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

describe('pull signals', () => {
  it('reads a settled state first, then a draft, then its checks', () => {
    expect(pullSignal(pull({ state: 'merged', draft: true }))).toBe('merged');
    expect(pullSignal(pull({ state: 'closed' }))).toBe('closed');
    expect(pullSignal(pull({ draft: true }))).toBe('draft');
    expect(pullSignal(pull({ checks: 'failing' }))).toBe('failing');
    expect(pullSignal(pull({ checks: 'pending' }))).toBe('running');
  });

  it('calls a status nobody could read unread rather than passing', () => {
    expect(pullSignal(pull(null))).toBe('unread');
    expect(pullSignal(pull({ checks: 'unknown' }))).toBe('blocked');
  });

  it('only says ready when the server said the whole pull request is', () => {
    expect(pullSignal(pull({}, true))).toBe('ready');
    expect(pullSignal(pull({}, false))).toBe('blocked');
    expect(pullSignal(pull({ threads: { open: 3, resolved: 0 } }, false))).toBe('blocked');
  });
});

describe('why an open pull request is not ready', () => {
  it('names the conflict, the comments and the checks', () => {
    expect(pullDetail(pull({ mergeable: 'conflicted' }))).toBe('Has merge conflicts');
    expect(pullDetail(pull({ mergeable: 'behind' }))).toBe('Behind its base branch');
    expect(pullDetail(pull({ threads: { open: 1, resolved: 0 } }))).toBe(
      '1 unresolved review comment'
    );
    expect(pullDetail(pull({ threads: { open: 4, resolved: 0 } }))).toBe(
      '4 unresolved review comments'
    );
    expect(pullDetail(pull({ checks: 'none' }))).toBe('No checks ran on this commit');
    expect(pullDetail(pull({ checks: 'unknown' }))).toBe('Checks could not be read');
    expect(pullDetail(pull({ mergeable: 'blocked' }))).toBe('Merging is blocked');
  });

  it('says review comments were unread rather than treating them as addressed', () => {
    expect(pullDetail(pull({ threads: null }))).toBe('Review comments could not be read');
  });

  it('gives a settled or ready pull request its own label', () => {
    expect(pullDetail(pull({}, true))).toBe('Ready to merge');
    expect(pullDetail(pull({ checks: 'failing' }))).toBe('Checks failing');
    expect(pullDetail(pull(null))).toBe('Pull request status unavailable');
  });
});

describe('the row title', () => {
  it('reads out the activity and the pull request together', () => {
    expect(
      indicatorTitle(
        indicator({
          activity: 'writing',
          pull_request: pull({ checks: 'failing' }),
          pull_requests: 1,
        })
      )
    ).toBe('Writing a reply · #12: Checks failing');
  });

  it('counts the pull requests the row is not speaking for', () => {
    expect(
      indicatorTitle(
        indicator({ activity: 'idle', pull_request: pull({}, true), pull_requests: 3 })
      )
    ).toBe('Idle · #12: Ready to merge (+2 more)');
  });

  it('says what an idle chat with no pull request is', () => {
    expect(indicatorTitle(indicator())).toBe('Idle');
  });
});

describe('ordering by what wants attention', () => {
  it('puts a person being waited on first, then a failing check, then a live turn', () => {
    const order = [
      indicator({ activity: 'idle', pull_request: pull({ state: 'merged' }) }),
      indicator({ activity: 'writing' }),
      indicator({ activity: 'awaiting_answer' }),
      indicator({ activity: 'idle', pull_request: pull({ checks: 'failing' }) }),
      indicator({ activity: 'awaiting_approval' }),
      indicator({ activity: 'idle', pull_request: pull({}, true) }),
    ];

    expect([...order].sort((a, b) => attentionRank(a) - attentionRank(b))).toEqual([
      order[4],
      order[2],
      order[3],
      order[1],
      order[5],
      order[0],
    ]);
  });

  it('sorts a chat with no reading yet last, so arriving readings do not reshuffle the top', () => {
    expect(attentionRank(undefined)).toBeGreaterThan(
      attentionRank(indicator({ activity: 'idle', pull_request: pull({ state: 'closed' }) }))
    );
  });

  it('ranks a failing pull request above the chat that is still writing', () => {
    expect(
      attentionRank(indicator({ activity: 'writing', pull_request: pull({ checks: 'failing' }) }))
    ).toBeLessThan(attentionRank(indicator({ activity: 'writing' })));
  });
});

describe('filtering', () => {
  it('keeps every chat under all, including ones with no reading', () => {
    expect(matchesFilter(undefined, 'all')).toBe(true);
    expect(matchesFilter(indicator(), 'all')).toBe(true);
  });

  it('matches a facet rather than the row’s most pressing signal', () => {
    const busyAndFailing = indicator({
      activity: 'writing',
      pull_request: pull({ checks: 'failing' }),
    });

    expect(matchesFilter(busyAndFailing, 'busy')).toBe(true);
    expect(matchesFilter(busyAndFailing, 'failing')).toBe(true);
    expect(matchesFilter(busyAndFailing, 'ready')).toBe(false);
  });

  it('counts both ways of waiting for a human as waiting', () => {
    expect(matchesFilter(indicator({ activity: 'awaiting_approval' }), 'waiting')).toBe(true);
    expect(matchesFilter(indicator({ activity: 'awaiting_answer' }), 'waiting')).toBe(true);
    expect(matchesFilter(indicator({ activity: 'writing' }), 'waiting')).toBe(false);
  });

  it('reads checks and readiness from the status, and claims neither without one', () => {
    expect(matchesFilter(indicator({ pull_request: pull({ checks: 'pending' }) }), 'running')).toBe(
      true
    );
    expect(matchesFilter(indicator({ pull_request: pull({}) }), 'passing')).toBe(true);
    expect(matchesFilter(indicator({ pull_request: pull({}, true) }), 'ready')).toBe(true);
    expect(matchesFilter(indicator({ pull_request: pull(null) }), 'passing')).toBe(false);
    expect(matchesFilter(indicator({ pull_request: pull(null) }), 'ready')).toBe(false);
    expect(matchesFilter(undefined, 'ready')).toBe(false);
  });

  it('knows which filters exist', () => {
    expect(isChatFilter('ready')).toBe(true);
    expect(isChatFilter('whatever')).toBe(false);
  });
});

describe('the remembered filter', () => {
  function withRefusingStorage(run: () => void): void {
    const { getItem, setItem } = localStorage;
    const refuse = () => {
      throw new Error('The operation is insecure.');
    };
    localStorage.getItem = refuse;
    localStorage.setItem = refuse;
    try {
      run();
    } finally {
      localStorage.getItem = getItem;
      localStorage.setItem = setItem;
    }
  }

  it('falls back to all chats when storage will not be read', () => {
    withRefusingStorage(() => {
      expect(readChatFilter()).toBe('all');
    });
  });

  it('applies for the session when storage will not be written', () => {
    withRefusingStorage(() => {
      expect(() => writeChatFilter('ready')).not.toThrow();
    });
  });

  it('round-trips a choice and ignores one it does not recognise', () => {
    writeChatFilter('failing');
    expect(readChatFilter()).toBe('failing');
    localStorage.setItem(CHAT_FILTER_STORAGE_KEY, 'whatever');
    expect(readChatFilter()).toBe('all');
    localStorage.removeItem(CHAT_FILTER_STORAGE_KEY);
  });
});

describe('looking indicators up by chat', () => {
  it('keys them by chat id', () => {
    const one = indicator({ chat_id: 'a' });
    const two = indicator({ chat_id: 'b', activity: 'writing' });

    expect(byChat([one, two])).toEqual({ a: one, b: two });
  });
});
