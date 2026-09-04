import type { Citation } from '../types';
import {
  citationEvidence,
  citationEvidenceLabel,
  citationHref,
  citationKindLabel,
  formatObservedAt,
  formatRevision,
} from '../utils/citations';

function CitationItem({ citation }: { citation: Citation }) {
  const href = citationHref(citation);
  const evidence = citationEvidence(citation);
  const revision = formatRevision(citation.revision);
  const external = Boolean(href && /^https?:\/\//i.test(href));
  const title = (
    <>
      <span className="citation-title">{citation.title}</span>
      <span className="citation-kind">{citationKindLabel(citation.kind)}</span>
    </>
  );

  return (
    <li className={`citation citation--${evidence}`} data-testid="citation">
      {href ? (
        <a
          className="citation-link"
          href={href}
          {...(external ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
        >
          {title}
        </a>
      ) : (
        <span className="citation-link citation-link--static">{title}</span>
      )}
      <span className="citation-meta">
        {revision ? <span className="citation-revision">{revision}</span> : null}
        <time className="citation-observed" dateTime={citation.observed_at}>
          {formatObservedAt(citation.observed_at)}
        </time>
        <span className="citation-evidence">{citationEvidenceLabel(evidence)}</span>
      </span>
      {citation.note ? <p className="citation-note">{citation.note}</p> : null}
    </li>
  );
}

export function Citations({ citations }: { citations: Citation[] }) {
  if (citations.length === 0) return null;

  return (
    <aside className="message-citations" aria-label="Sources" data-testid="citations">
      <h4 className="citations-heading">Sources</h4>
      <ol className="citation-list">
        {citations.map((citation) => (
          <CitationItem
            key={`${citation.kind}:${citation.url}:${citation.revision ?? ''}:${citation.observed_at}`}
            citation={citation}
          />
        ))}
      </ol>
    </aside>
  );
}
