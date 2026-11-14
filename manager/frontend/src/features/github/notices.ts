import type { SourceStatus } from './sourceStatus';

export const UNAVAILABLE = 'GitHub is not answering. Retrying shortly.';

export interface Listed {
  stale: boolean;
  partial: boolean;
  warnings: readonly string[];
  sources: readonly SourceStatus[];
}

export interface NoticeTexts {
  stale: string;
  partial: string;
}

export function noticesOf(listing: Listed | undefined, texts: NoticeTexts): string[] {
  if (!listing) return [];
  const notices = new Set<string>();
  if (listing.stale) notices.add(texts.stale);
  if (listing.partial) notices.add(texts.partial);
  for (const warning of listing.warnings) notices.add(warning);
  for (const source of listing.sources) {
    if (source.warning) notices.add(`${source.repository}: ${source.warning}`);
  }
  return [...notices];
}
