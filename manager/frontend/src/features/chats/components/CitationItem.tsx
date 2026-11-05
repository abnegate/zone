import type { Citation } from '../types';
import {
  citationEvidence,
  citationEvidenceLabel,
  citationHref,
  citationKindLabel,
  citationProvenanceLabel,
  formatCitationTimes,
  isRevisionTimestamp,
} from '../utils/citations';

export interface CitationItemProps {
  citation: Citation;
  sourceKey: string;
  highlighted?: boolean;
}

export function CitationItem({ citation, sourceKey, highlighted = false }: CitationItemProps) {
  const href = citationHref(citation);
  const evidence = citationEvidence(citation);
  const times = formatCitationTimes(citation);
  const revisionClass = isRevisionTimestamp(citation.revision)
    ? 'citation-revision citation-revision--time'
    : 'citation-revision';
  const claim = citationProvenanceLabel(citation.provenance);
  const external = Boolean(href && /^https?:\/\//i.test(href));
  const title = (
    <>
      <span className="citation-title">{citation.title}</span>
      <span className="citation-kind">{citationKindLabel(citation.kind)}</span>
    </>
  );
  const classes = [
    'citation',
    `citation--${evidence}`,
    `citation--${citation.provenance.replace('_', '-')}`,
    highlighted ? 'is-highlighted' : null,
  ];

  return (
    <li
      className={classes.filter(Boolean).join(' ')}
      data-testid="citation"
      data-source={sourceKey}
    >
      {href ? (
        <a
          className="citation-link"
          href={href}
          {...(external ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
        >
          {title}
        </a>
      ) : (
        <span className="citation-link citation-link--static" tabIndex={-1}>
          {title}
        </span>
      )}
      <span className="citation-meta">
        {times.revision ? <span className={revisionClass}>{times.revision}</span> : null}
        <span className="citation-observation">
          <span className="citation-evidence">{citationEvidenceLabel(evidence)}</span>{' '}
          <time className="citation-observed" dateTime={citation.observed_at}>
            {times.observed}
          </time>
        </span>
        {claim ? <span className="citation-provenance">{claim}</span> : null}
      </span>
      {citation.note ? <p className="citation-note">{citation.note}</p> : null}
    </li>
  );
}
