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
  web: 'Web page',
  behavioral_verification: 'Behavioral verification',
};

export function citationKindLabel(kind: Citation['kind']): string {
  return KIND_LABELS[kind] ?? kind;
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

export function citationHref(citation: Pick<Citation, 'url' | 'kind'>): string | null {
  if (ABSOLUTE_URL.test(citation.url) || IN_APP_PATH.test(citation.url)) return citation.url;
  if (citation.kind === 'workspace_document' || citation.url.startsWith('knowledge://')) {
    return '/wiki';
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

export function mergeCitations(existing: Citation[] | undefined, incoming: Citation[]): Citation[] {
  const merged = [...(existing ?? [])];
  for (const citation of incoming) {
    if (merged.some((seen) => seen.url === citation.url && seen.revision === citation.revision)) {
      continue;
    }
    merged.push(citation);
  }
  return merged;
}
