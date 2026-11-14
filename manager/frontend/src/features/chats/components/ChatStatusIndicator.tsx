import type { ReactNode } from 'react';
import type { ChatIndicator } from '../types';
import {
  ACTIVITY_LABELS,
  indicatorTitle,
  type PullSignal,
  pullDetail,
  pullSignal,
} from '../utils/indicators';

const PULL_REQUEST = (
  <>
    <circle cx="18" cy="18" r="3" />
    <circle cx="6" cy="6" r="3" />
    <path d="M13 6h3a2 2 0 0 1 2 2v7M6 9v12" />
  </>
);

/**
 * One glyph per signal, sharing the shapes the changes panel uses so the same
 * state does not look like two different things in two places.
 *
 * `ready` is the one shape that is only in the list: a merge arrow, because
 * "every check passed" and "this can go in now" are different claims and the
 * row has to be able to make the second one.
 */
const SHAPES: Record<PullSignal, ReactNode> = {
  failing: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="m15 9-6 6M9 9l6 6" />
    </>
  ),
  running: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </>
  ),
  blocked: PULL_REQUEST,
  ready: (
    <>
      <path d="M6 3v8a5 5 0 0 0 5 5h8" />
      <path d="m16 13 3 3-3 3" />
    </>
  ),
  unread: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 16h.01M9.5 9a2.5 2.5 0 1 1 3.6 2.3c-.7.4-1.1 1-1.1 1.7" />
    </>
  ),
  draft: (
    <>
      <circle cx="18" cy="18" r="3" />
      <circle cx="6" cy="6" r="3" />
      <path d="M18 6V5M18 11v-1M6 9v12" />
    </>
  ),
  merged: (
    <>
      <circle cx="18" cy="18" r="3" />
      <circle cx="6" cy="6" r="3" />
      <path d="M6 21V9a9 9 0 0 0 9 9" />
    </>
  ),
  closed: (
    <>
      <circle cx="6" cy="6" r="3" />
      <circle cx="18" cy="18" r="3" />
      <path d="M6 9v12M21 3l-6 6M21 9l-6-6M18 11.5V15" />
    </>
  ),
};

/**
 * What one chat's row says it is doing, and what became of its pull request.
 *
 * Two glyphs rather than one, because a chat can be writing a reply while the
 * pull request it opened earlier is failing, and collapsing that into a single
 * worst-of signal would hide one of them. An idle chat with no pull request
 * shows nothing: an indicator for every row would make none of them legible.
 */
export function ChatStatusIndicator({ indicator }: { indicator?: ChatIndicator }) {
  const pull = indicator?.pull_request ?? null;
  const signal = pull ? pullSignal(pull) : null;
  const activity = indicator?.activity ?? 'idle';
  if (!indicator || (activity === 'idle' && !signal)) return null;

  return (
    <span className="chat-status" title={indicatorTitle(indicator)} data-testid="chat-status">
      {activity !== 'idle' && (
        <span
          className={`chat-status-activity is-${activity}`}
          role="img"
          aria-label={ACTIVITY_LABELS[activity]}
          data-testid={`chat-status-${activity}`}
        />
      )}
      {pull && signal && (
        <svg
          className={`chat-status-pull is-${signal}`}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.25"
          strokeLinecap="round"
          strokeLinejoin="round"
          role="img"
          aria-label={`Pull request #${pull.number}: ${pullDetail(pull)}`}
          data-testid={`chat-status-pull-${signal}`}
        >
          <title>{`Pull request #${pull.number}: ${pullDetail(pull)}`}</title>
          {SHAPES[signal]}
        </svg>
      )}
    </span>
  );
}
