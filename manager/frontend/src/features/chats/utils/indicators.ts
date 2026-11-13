import type { ChatIndicator, ChatPullIndicator } from '../types';

/**
 * What a chat's row says about its pull request.
 *
 * `unread` is the one that matters most to get right: GitHub could not be
 * asked, so the row says so rather than borrowing the look of a passing one.
 */
export const PULL_SIGNALS = [
  'failing',
  'running',
  'blocked',
  'ready',
  'unread',
  'draft',
  'merged',
  'closed',
] as const;
export type PullSignal = (typeof PULL_SIGNALS)[number];

export const PULL_SIGNAL_LABELS: Record<PullSignal, string> = {
  failing: 'Checks failing',
  running: 'Checks running',
  blocked: 'Not ready to merge',
  ready: 'Ready to merge',
  unread: 'Pull request status unavailable',
  draft: 'Draft pull request',
  merged: 'Merged',
  closed: 'Closed without merging',
};

/**
 * What a chat's row says about the chat itself, as a word for a reader rather
 * than the server's own name for it.
 */
export const ACTIVITY_LABELS = {
  writing: 'Writing a reply',
  awaiting_approval: 'Waiting for you to approve a tool call',
  awaiting_answer: 'Waiting for your answer',
  idle: 'Idle',
} as const;

/** What a reader can narrow the list down to. */
export const CHAT_FILTERS = [
  'all',
  'busy',
  'waiting',
  'failing',
  'running',
  'passing',
  'ready',
] as const;
export type ChatFilter = (typeof CHAT_FILTERS)[number];

export const CHAT_FILTER_OPTIONS: ReadonlyArray<{ value: ChatFilter; label: string }> = [
  { value: 'all', label: 'All chats' },
  { value: 'busy', label: 'Busy' },
  { value: 'waiting', label: 'Waiting for a human' },
  { value: 'failing', label: 'Checks failing' },
  { value: 'running', label: 'Checks running' },
  { value: 'passing', label: 'Checks passing' },
  { value: 'ready', label: 'Ready to merge' },
];

export const CHAT_FILTER_STORAGE_KEY = 'manager_chats_filter';

export function isChatFilter(value: string): value is ChatFilter {
  return (CHAT_FILTERS as readonly string[]).includes(value);
}

export function readChatFilter(): ChatFilter {
  const value = localStorage.getItem(CHAT_FILTER_STORAGE_KEY);
  return value !== null && isChatFilter(value) ? value : 'all';
}

export function writeChatFilter(value: ChatFilter): void {
  localStorage.setItem(CHAT_FILTER_STORAGE_KEY, value);
}

/**
 * Which signal a pull request shows.
 *
 * Readiness comes from the server, which reads it from the whole of the pull
 * request: checks, conflicts and unresolved review threads. An open pull
 * request that is not ready is `blocked` whatever is in the way, and the title
 * carries the detail.
 */
export function pullSignal(pull: ChatPullIndicator): PullSignal {
  const status = pull.status;
  if (!status) return 'unread';
  if (status.state === 'merged') return 'merged';
  if (status.state === 'closed') return 'closed';
  if (status.draft) return 'draft';
  if (status.checks === 'failing') return 'failing';
  if (status.checks === 'pending') return 'running';
  return pull.ready ? 'ready' : 'blocked';
}

/** Why an open pull request is not ready, in the order a reader can act on. */
export function pullDetail(pull: ChatPullIndicator): string {
  const status = pull.status;
  const signal = pullSignal(pull);
  if (!status || signal !== 'blocked') return PULL_SIGNAL_LABELS[signal];

  if (status.mergeable === 'conflicted') return 'Has merge conflicts';
  if (status.mergeable === 'behind') return 'Behind its base branch';
  if (!status.threads) return 'Review comments could not be read';
  if (status.threads.open > 0) {
    return status.threads.open === 1
      ? '1 unresolved review comment'
      : `${status.threads.open} unresolved review comments`;
  }
  if (status.checks === 'none') return 'No checks ran on this commit';
  if (status.checks === 'unknown') return 'Checks could not be read';
  if (status.mergeable === 'blocked') return 'Merging is blocked';
  return PULL_SIGNAL_LABELS.blocked;
}

/** The whole of what one row's indicators say, for its tooltip. */
export function indicatorTitle(indicator: ChatIndicator): string {
  const parts: string[] = [ACTIVITY_LABELS[indicator.activity]];
  if (indicator.pull_request) {
    const detail = pullDetail(indicator.pull_request);
    const extra = indicator.pull_requests > 1 ? ` (+${indicator.pull_requests - 1} more)` : '';
    parts.push(`#${indicator.pull_request.number}: ${detail}${extra}`);
  }
  return parts.join(' · ');
}

/**
 * Where a chat comes in the queue for a reader's attention, lowest first.
 *
 * A person who is being waited on comes before anything a machine is doing,
 * because the chat has stopped until they answer. Then a failing pull request,
 * then the rest in the order a reader would work through them. A chat with no
 * indicator at all sorts last rather than first, so an unloaded list does not
 * reorder itself as readings arrive.
 */
export function attentionRank(indicator: ChatIndicator | undefined): number {
  if (!indicator) return 100;
  if (indicator.activity === 'awaiting_approval') return 0;
  if (indicator.activity === 'awaiting_answer') return 1;
  const pull = indicator.pull_request;
  const signal = pull ? pullSignal(pull) : null;
  if (signal === 'failing') return 2;
  if (indicator.activity === 'writing') return 3;
  switch (signal) {
    case 'running':
      return 4;
    case 'blocked':
      return 5;
    case 'ready':
      return 6;
    case 'draft':
      return 7;
    case 'unread':
      return 8;
    case 'merged':
      return 9;
    case 'closed':
      return 10;
    default:
      return 11;
  }
}

/**
 * Whether a chat belongs in the list under this filter.
 *
 * A filter asks about one facet rather than about the row's single most
 * pressing signal: a chat writing a reply with a failing pull request is in
 * both "Busy" and "Checks failing", because both are true of it. A chat with no
 * reading yet matches nothing but "All chats", since claiming otherwise would
 * be claiming a status nobody read.
 */
export function matchesFilter(indicator: ChatIndicator | undefined, filter: ChatFilter): boolean {
  if (filter === 'all') return true;
  if (!indicator) return false;
  switch (filter) {
    case 'busy':
      return indicator.activity === 'writing';
    case 'waiting':
      return indicator.activity === 'awaiting_approval' || indicator.activity === 'awaiting_answer';
    case 'failing':
      return indicator.pull_request?.status?.checks === 'failing';
    case 'running':
      return indicator.pull_request?.status?.checks === 'pending';
    case 'passing':
      return indicator.pull_request?.status?.checks === 'passing';
    case 'ready':
      return indicator.pull_request?.ready === true;
  }
}

/** The indicators by chat id, as the list looks them up. */
export function byChat(indicators: readonly ChatIndicator[]): Record<string, ChatIndicator> {
  return Object.fromEntries(indicators.map((indicator) => [indicator.chat_id, indicator]));
}
