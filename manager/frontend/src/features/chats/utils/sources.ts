import type { Citation, Message } from '../types';

export interface ChatSource {
  key: string;
  messageId: string;
  citation: Citation;
}

/** Which sources the panel should bring into view, as a fresh object per request. */
export interface SourceRequest {
  keys: readonly string[];
}

export function sourceKey(citation: Citation): string {
  return citation.url || citation.identifier || citation.title;
}

function observedTime(citation: Citation): number {
  const time = Date.parse(citation.observed_at);
  return Number.isNaN(time) ? Number.NEGATIVE_INFINITY : time;
}

/**
 * Every source the chat's replies cite, newest first. A source cited again
 * shows once, as it was last observed, since that is what is true of it now.
 */
export function chatSources(messages: readonly Message[]): ChatSource[] {
  const latest = new Map<string, ChatSource & { order: number }>();
  let order = 0;
  for (const message of messages) {
    for (const citation of message.metadata?.citations ?? []) {
      const key = sourceKey(citation);
      const known = latest.get(key);
      order += 1;
      if (known && observedTime(known.citation) > observedTime(citation)) continue;
      latest.set(key, { key, messageId: message.id, citation, order });
    }
  }
  return Array.from(latest.values())
    .sort(
      (left, right) =>
        observedTime(right.citation) - observedTime(left.citation) || right.order - left.order
    )
    .map(({ key, messageId, citation }) => ({ key, messageId, citation }));
}

export function sourceKeys(citations: readonly Citation[]): string[] {
  return Array.from(new Set(citations.map(sourceKey)));
}
