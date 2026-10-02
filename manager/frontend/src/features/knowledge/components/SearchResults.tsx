import { Badge } from '@zone/ui';
import DOMPurify from 'dompurify';
import type { SearchResult } from '../types';

const FileIcon = () => (
  <svg
    width="12"
    height="12"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" />
    <path d="M14 2v4a2 2 0 0 0 2 2h4" />
  </svg>
);

const FolderIcon = () => (
  <svg
    width="14"
    height="14"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
  </svg>
);

type RelevanceLevel = 'high' | 'medium' | 'low';

const relevanceLevel = (score: number): RelevanceLevel => {
  if (score >= 0.8) return 'high';
  if (score >= 0.5) return 'medium';
  return 'low';
};

const RELEVANCE_LABELS: Record<RelevanceLevel, string> = {
  high: 'Highly relevant',
  medium: 'Relevant',
  low: 'Partial match',
};

const RELEVANCE_VARIANTS: Record<RelevanceLevel, 'success' | 'warning' | 'neutral'> = {
  high: 'success',
  medium: 'warning',
  low: 'neutral',
};

const escapeRegex = (str: string) => str.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

export function highlightText(text: string, query: string): string {
  const queryTerms = query.toLowerCase().split(/\s+/);
  let highlighted = text
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#039;');

  queryTerms.forEach((term) => {
    if (term.length > 2) {
      const regex = new RegExp(`(${escapeRegex(term)})`, 'gi');
      highlighted = highlighted.replace(regex, '<mark>$1</mark>');
    }
  });

  return DOMPurify.sanitize(highlighted);
}

function resultScoreLabel(result: SearchResult): string {
  const semantic = result.metadata.semantic_score;
  if (typeof semantic === 'number') {
    return `${Math.round(semantic * 100)}% semantic`;
  }
  if (typeof result.metadata.keyword_score === 'number') {
    return 'Keyword match';
  }
  return RELEVANCE_LABELS[relevanceLevel(result.relevance_score)];
}

type SearchResultsProps = {
  results: SearchResult[];
  total: number;
  query: string;
};

export default function SearchResults({ results, total, query }: SearchResultsProps) {
  return (
    <div className="results-section">
      <div className="results-header">
        <h2 className="results-title">In sources</h2>
        <Badge variant="neutral">{total} found</Badge>
      </div>

      <div className="results-grid">
        {results.map((result) => {
          const level = relevanceLevel(result.relevance_score);
          const path = typeof result.metadata.path === 'string' ? result.metadata.path : '';
          return (
            <article key={result.id} className="card card--list result-card">
              <div className="result-card-header">
                <span className="result-source">
                  <FolderIcon />
                  <span className="result-source-name">{result.source_name}</span>
                </span>
                <Badge variant={RELEVANCE_VARIANTS[level]}>{resultScoreLabel(result)}</Badge>
              </div>

              <div
                className="result-snippet"
                // biome-ignore lint/security/noDangerouslySetInnerHtml: Sanitized with DOMPurify
                dangerouslySetInnerHTML={{ __html: highlightText(result.snippet, query) }}
              />

              {path && (
                <div className="result-meta">
                  <FileIcon />
                  <span className="result-path">{path}</span>
                </div>
              )}
            </article>
          );
        })}
      </div>
    </div>
  );
}
