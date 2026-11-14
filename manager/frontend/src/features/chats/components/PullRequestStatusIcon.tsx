import type { ReactNode } from 'react';
import { PULL_REQUEST_KIND_LABELS, type PullRequestKind } from '../utils/pullRequests';

const PULL_REQUEST = (
  <>
    <circle cx="18" cy="18" r="3" />
    <circle cx="6" cy="6" r="3" />
    <path d="M13 6h3a2 2 0 0 1 2 2v7M6 9v12" />
  </>
);

const SHAPES: Record<PullRequestKind, ReactNode> = {
  passing: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="m8.5 12 2.5 2.5 4.5-5" />
    </>
  ),
  conflicted: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="m8 8 3 4-3 4M16 8l-3 4 3 4" />
    </>
  ),
  failing: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="m15 9-6 6M9 9l6 6" />
    </>
  ),
  pending: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
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
  draft: (
    <>
      <circle cx="18" cy="18" r="3" />
      <circle cx="6" cy="6" r="3" />
      <path d="M18 6V5M18 11v-1M6 9v12" />
    </>
  ),
  open: PULL_REQUEST,
  unknown: PULL_REQUEST,
};

export function PullRequestStatusIcon({ kind }: { kind: PullRequestKind }) {
  return (
    <svg
      className={`chat-change-icon is-${kind}`}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      role="img"
      aria-label={PULL_REQUEST_KIND_LABELS[kind]}
    >
      <title>{PULL_REQUEST_KIND_LABELS[kind]}</title>
      {SHAPES[kind]}
    </svg>
  );
}
