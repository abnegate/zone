import type { Citation } from '../types';

/// Only an absolute http(s) URL has an origin the server could have cited. A
/// relative path, a protocol-relative `//host`, a `mailto:` and the empty href
/// react-markdown leaves behind after blanking `javascript:` all canonicalise
/// to null, and null is never equal to a citation.
export function canonicalUrl(value: string | null | undefined): string | null {
  if (!value) return null;

  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return null;
  }

  if (url.protocol !== 'http:' && url.protocol !== 'https:') return null;

  const path = url.pathname === '/' ? '' : url.pathname;
  return `${url.protocol}//${url.host}${path}${url.search}`;
}

export function resolvesToCitation(
  href: string | null | undefined,
  citations: readonly Citation[]
): boolean {
  const target = canonicalUrl(href);
  if (target === null) return false;
  return citations.some((citation) => canonicalUrl(citation.url) === target);
}
