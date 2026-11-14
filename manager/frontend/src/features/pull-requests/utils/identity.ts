import type { PullRequestReference } from '../types';

const SEPARATOR = ':';

const WHOLE_NUMBER = /^[1-9]\d*$/;

export function pullRequestKey(reference: PullRequestReference): string {
  return `${reference.source_id}${SEPARATOR}${reference.number}`;
}

export function parsePullRequestKey(value: string | null | undefined): PullRequestReference | null {
  if (!value) return null;
  const split = value.lastIndexOf(SEPARATOR);
  if (split <= 0) return null;
  const sourceId = value.slice(0, split);
  const number = value.slice(split + SEPARATOR.length);
  if (!WHOLE_NUMBER.test(number)) return null;
  const parsed = Number(number);
  return Number.isSafeInteger(parsed) ? { source_id: sourceId, number: parsed } : null;
}

export function samePullRequest(left: PullRequestReference, right: PullRequestReference): boolean {
  return left.source_id === right.source_id && left.number === right.number;
}
