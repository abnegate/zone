import type { Citation } from '../types';

export type CitationEvidence = 'passing' | 'failed' | 'pending' | 'incomplete' | 'observed';

const KIND_LABELS: Record<Citation['kind'], string> = {
  github_build: 'GitHub build',
  github_deployment: 'GitHub deployment',
  github_issue: 'GitHub issue',
  github_file: 'GitHub file',
  workspace_document: 'Workspace document',
};

export function citationKindLabel(kind: Citation['kind']): string {
  return KIND_LABELS[kind] ?? kind;
}

/// Passing only when the observation is complete and successful.
export function citationEvidence(citation: Citation): CitationEvidence {
  if (citation.outcome === 'success' && citation.complete) return 'passing';
  if (citation.outcome === 'failure' && citation.complete) return 'failed';
  if (citation.outcome === 'pending' && citation.complete) return 'pending';
  if (citation.outcome === 'observed' && citation.complete) return 'observed';
  return 'incomplete';
}

export function citationEvidenceLabel(evidence: CitationEvidence): string {
  switch (evidence) {
    case 'passing':
      return 'Passing';
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

export function citationHref(citation: Pick<Citation, 'url' | 'kind'>): string | null {
  if (/^https?:\/\//i.test(citation.url) || citation.url.startsWith('/')) return citation.url;
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
