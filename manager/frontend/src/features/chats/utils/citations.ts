import type { Citation, CitationProvenance } from '../types';

export type CitationEvidence =
  | 'passing'
  | 'claimed'
  | 'failed'
  | 'pending'
  | 'incomplete'
  | 'observed';

const KIND_LABELS: Record<Citation['kind'], string> = {
  github_build: 'GitHub build',
  github_deployment: 'GitHub deployment',
  github_issue: 'GitHub issue',
  github_file: 'GitHub file',
  workspace_document: 'Workspace document',
  knowledge_passage: 'Knowledge passage',
  web: 'Web page',
  behavioral_verification: 'Behavioral verification',
};

export function citationKindLabel(kind: Citation['kind']): string {
  return KIND_LABELS[kind] ?? kind;
}

const ANCHOR_PREFIX = 'citation-';

/// The id the citations aside puts on a chip, and the target a source marker in
/// the reply links to. Both sides derive it from the identifier alone, so a
/// reordered citation list never re-points a marker.
export function citationAnchorId(identifier: string): string {
  return `${ANCHOR_PREFIX}${identifier
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')}`;
}

/// Passing only when the observation is complete, successful, and something
/// other than the model saw it. The server's own `passing()` requires all
/// three; dropping the third here would render a claim as proof in the one
/// place a human actually reads the verdict.
export function citationEvidence(citation: Citation): CitationEvidence {
  if (citation.outcome === 'success' && citation.complete) {
    return citation.provenance === 'server_execution' ? 'passing' : 'claimed';
  }
  if (citation.outcome === 'failure' && citation.complete) return 'failed';
  if (citation.outcome === 'pending' && citation.complete) return 'pending';
  if (citation.outcome === 'observed' && citation.complete) return 'observed';
  return 'incomplete';
}

export function citationEvidenceLabel(evidence: CitationEvidence): string {
  switch (evidence) {
    case 'passing':
      return 'Passing';
    case 'claimed':
      return 'Claimed';
    case 'failed':
      return 'Failed';
    case 'pending':
      return 'Pending';
    case 'observed':
      return 'Observed';
    default:
      return 'Incomplete evidence';
  }
}

/// Server-proven evidence is the norm and gets no chrome; a model's claim is
/// the exception a reader has to know about.
export function citationProvenanceLabel(provenance: CitationProvenance): string | null {
  return provenance === 'model_asserted' ? 'Claimed by the model, not verified' : null;
}

const ABSOLUTE_URL = /^https?:\/\//i;

/// One leading slash is an in-app route. A second slash — or a backslash, which
/// browsers fold into one — makes the url protocol-relative, and the caller reads
/// that off-site destination as internal and renders it without `noopener`.
const IN_APP_PATH = /^\/(?![/\\])/;

const KNOWLEDGE_SCHEME = 'knowledge://';
const WIKI_PATH = '/wiki';

/// A knowledge entry id is a UUID column, so any other suffix names no entry.
const ENTRY_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/// The wiki opens the entry named by `?id=`, so a knowledge entry citation
/// lands on the passage a reader came to check rather than on the index.
export function citationHref(citation: Pick<Citation, 'url' | 'kind'>): string | null {
  if (ABSOLUTE_URL.test(citation.url) || IN_APP_PATH.test(citation.url)) return citation.url;
  if (citation.url.startsWith(KNOWLEDGE_SCHEME)) {
    const entry = citation.url.slice(KNOWLEDGE_SCHEME.length);
    return ENTRY_ID.test(entry) ? `${WIKI_PATH}?id=${encodeURIComponent(entry)}` : WIKI_PATH;
  }
  if (citation.kind === 'workspace_document' || citation.kind === 'knowledge_passage') {
    return WIKI_PATH;
  }
  return null;
}

const COMMIT_SHA = /^[0-9a-f]{40}$/i;
const ISO_TIMESTAMP = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?(?:Z|[+-]\d{2}:?\d{2})?$/;
const ZONED = /(?:Z|[+-]\d{2}:?\d{2})$/;
const REVISED_LABEL = 'Revised';

export interface CitationTimes {
  revision: string | null;
  observed: string;
}

export function isRevisionTimestamp(revision?: string | null): boolean {
  return Boolean(revision && ISO_TIMESTAMP.test(revision));
}

/// A zoneless stamp is the server's own clock, which keeps UTC.
function parseTimestamp(value: string): Date {
  return new Date(ZONED.test(value) ? value : `${value}Z`);
}

function validDate(date: Date): Date | null {
  return Number.isNaN(date.getTime()) ? null : date;
}

function sameDay(left: Date, right: Date): boolean {
  return (
    left.getFullYear() === right.getFullYear() &&
    left.getMonth() === right.getMonth() &&
    left.getDate() === right.getDate()
  );
}

function formatTime(date: Date): string {
  return date.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
}

/// The year is noise while it is the current one, which is nearly always.
function formatStamp(date: Date, now: Date): string {
  return date.toLocaleString([], {
    month: 'short',
    day: 'numeric',
    ...(date.getFullYear() === now.getFullYear() ? {} : { year: 'numeric' }),
    hour: 'numeric',
    minute: '2-digit',
  });
}

/// A git source is named by its commit, shortened the way git shows it. A
/// document's revision is the time it was last edited, which reads as a date
/// beside the observation time — or not at all when the two would print the
/// same minute, since a reader gains nothing from seeing it twice.
function formatRevision(
  revision: string | null | undefined,
  revised: Date | null,
  observed: Date | null,
  now: Date
): string | null {
  if (!revision) return null;
  if (COMMIT_SHA.test(revision)) return revision.slice(0, 7);
  if (!revised) return revision;
  const label = formatStamp(revised, now);
  if (observed && label === formatStamp(observed, now)) return null;
  return `${REVISED_LABEL} ${label}`;
}

/// The two stamps on a citation are read together, so the observation drops the
/// date it shares with the revision printed just before it.
export function formatCitationTimes(
  citation: Pick<Citation, 'revision' | 'observed_at'>,
  now: Date = new Date()
): CitationTimes {
  const observed = validDate(new Date(citation.observed_at));
  const revised = isRevisionTimestamp(citation.revision)
    ? validDate(parseTimestamp(citation.revision ?? ''))
    : null;
  const revision = formatRevision(citation.revision, revised, observed, now);
  if (!observed) return { revision, observed: citation.observed_at };
  const dated = !(revision && revised && sameDay(revised, observed));
  return { revision, observed: dated ? formatStamp(observed, now) : formatTime(observed) };
}

export function formatObservedAt(value: string, now: Date = new Date()): string {
  const date = validDate(new Date(value));
  return date ? formatStamp(date, now) : value;
}

/// An identifier names a source; an address only says where to read one. One
/// address can back several registry sources — a document indexed into the
/// knowledge base is reachable as both — so folding them together by address
/// would drop an identifier the reply already cites and leave its marker
/// reading as unresolved. Address equality settles it only for citations
/// stored before identifiers existed.
function sameSource(seen: Citation, incoming: Citation): boolean {
  const left = handle(seen);
  const right = handle(incoming);
  if (left && right) return left === right;
  return seen.url === incoming.url && seen.revision === incoming.revision;
}

function handle(citation: Citation): string | null {
  return citation.identifier?.trim().toLowerCase() || null;
}

/// One source retrieved twice in a turn is one citation, and it has to read as
/// the most the server actually saw. A document listed without its content and
/// then read in full is complete evidence: leaving the listing's citation in
/// place tells the reader the content was unavailable for a passage the reply
/// is quoting. Only an incomplete citation gives way, so a later listing never
/// takes back what a read proved, and the observation keeps its first time. It
/// also keeps the identifier it already had, because a source can match by
/// address alone and the reply may already be citing that handle.
export function mergeCitations(existing: Citation[] | undefined, incoming: Citation[]): Citation[] {
  const merged = [...(existing ?? [])];
  for (const citation of incoming) {
    const seen = merged.findIndex((known) => sameSource(known, citation));
    if (seen === -1) {
      merged.push(citation);
      continue;
    }
    const known = merged[seen];
    if (!known.complete && citation.complete) {
      merged[seen] = {
        ...citation,
        observed_at: known.observed_at,
        identifier: known.identifier ?? citation.identifier,
      };
    }
  }
  return merged;
}
