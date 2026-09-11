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

/// The wiki opens the entry named by `?id=`, so a knowledge entry citation
/// lands on the passage a reader came to check rather than on the index.
export function citationHref(citation: Pick<Citation, 'url' | 'kind'>): string | null {
  if (ABSOLUTE_URL.test(citation.url) || IN_APP_PATH.test(citation.url)) return citation.url;
  if (citation.url.startsWith(KNOWLEDGE_SCHEME)) {
    const entry = citation.url.slice(KNOWLEDGE_SCHEME.length);
    return entry ? `${WIKI_PATH}?id=${encodeURIComponent(entry)}` : WIKI_PATH;
  }
  if (citation.kind === 'workspace_document' || citation.kind === 'knowledge_passage') {
    return WIKI_PATH;
  }
  return null;
}

export function formatRevision(revision?: string | null): string | null {
  if (!revision) return null;
  if (/^[0-9a-f]{40}$/i.test(revision)) return revision.slice(0, 7);
  return revision;
}

export function formatObservedAt(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' });
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

export function mergeCitations(existing: Citation[] | undefined, incoming: Citation[]): Citation[] {
  const merged = [...(existing ?? [])];
  for (const citation of incoming) {
    if (merged.some((seen) => sameSource(seen, citation))) continue;
    merged.push(citation);
  }
  return merged;
}
