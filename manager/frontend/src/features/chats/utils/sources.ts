import type { Citation, Message } from '../types';

export interface ChatSource {
  key: string;
  messageId: string;
  citation: Citation;
}

export interface ChatSources {
  all: ChatSource[];
  replies: ReadonlyMap<string, ChatSource[]>;
}

/** A fresh object per request: the panel acts on each one once, by identity. */
export interface SourceRequest {
  messageId: string;
  key: string | null;
}

interface Observed extends ChatSource {
  time: number;
  order: number;
}

function bare({ key, messageId, citation }: Observed): ChatSource {
  return { key, messageId, citation };
}

const observedTimes = new WeakMap<Citation, number>();
interface Reply {
  messageId: string;
  observed: Observed[];
  sources: ChatSource[];
}

const replyCache = new WeakMap<readonly Citation[], Reply>();

export function sourceKey(citation: Citation): string {
  return citation.url || citation.identifier || citation.title;
}

function observedTime(citation: Citation): number {
  const cached = observedTimes.get(citation);
  if (cached !== undefined) return cached;
  const parsed = Date.parse(citation.observed_at);
  const time = Number.isNaN(parsed) ? Number.NEGATIVE_INFINITY : parsed;
  observedTimes.set(citation, time);
  return time;
}

function newestFirst(left: Observed, right: Observed): number {
  return right.time - left.time || right.order - left.order;
}

function latest(entries: Iterable<Observed>): Observed[] {
  const kept = new Map<string, Observed>();
  for (const entry of entries) {
    const known = kept.get(entry.key);
    if (!known || entry.time >= known.time) kept.set(entry.key, entry);
  }
  return Array.from(kept.values()).sort(newestFirst);
}

function reply(message: Message, citations: readonly Citation[]): Reply {
  const cached = replyCache.get(citations);
  if (cached?.messageId === message.id) return cached;
  const observed = latest(
    citations.map((citation, order) => ({
      key: sourceKey(citation),
      messageId: message.id,
      citation,
      time: observedTime(citation),
      order,
    }))
  );
  const built = { messageId: message.id, observed, sources: observed.map(bare) };
  replyCache.set(citations, built);
  return built;
}

/**
 * The whole chat's sources, each as last observed, and each reply's own
 * sources as they stood when that reply cited them: a later observation must
 * never stand in for the evidence an earlier reply was written on.
 */
export function chatSources(messages: readonly Message[]): ChatSources {
  const replies = new Map<string, ChatSource[]>();
  const everything: Observed[] = [];
  let offset = 0;
  for (const message of messages) {
    const citations = message.metadata?.citations;
    if (!citations?.length) continue;
    const { observed, sources } = reply(message, citations);
    replies.set(message.id, sources);
    for (const entry of observed) {
      everything.push({ ...entry, order: offset + entry.order });
    }
    offset += citations.length;
  }
  return { all: latest(everything).map(bare), replies };
}
