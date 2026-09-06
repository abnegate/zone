import { useId, useRef, useState } from 'react';
import type { ContextUsage as Usage } from '../types';
import './ContextUsage.css';

interface Props {
  usage: Usage | null;
  error?: string | null;
  previewing?: boolean;
}
const labels: Record<keyof Usage['breakdown'], string> = {
  instructions: 'Instructions',
  conversation: 'Conversation',
  tools: 'Tool definitions',
  results: 'Tool results',
  summary: 'Retained summary',
  attachments: 'Attachments',
  overhead: 'Message overhead',
};
const count = (value: number | null): string =>
  value === null ? 'Unknown' : value.toLocaleString();

export function ContextUsage({ usage, error, previewing = false }: Props) {
  const [expanded, setExpanded] = useState(false);
  const identifier = useId();
  const button = useRef<HTMLButtonElement>(null);
  const percentage =
    usage && usage.limit !== null && usage.limit > 0 && !usage.incomplete
      ? Math.round((100 * usage.used) / usage.limit)
      : null;
  const status =
    usage?.status === 'compacting'
      ? 'Compacting…'
      : usage?.status === 'compacted'
        ? 'Compacted'
        : usage?.status === 'blocked'
          ? 'Needs attention'
          : usage?.status === 'unavailable'
            ? 'Unavailable'
            : usage?.status === 'ready' && usage.threshold !== null && usage.remaining === 0
              ? 'Will compact before sending'
              : previewing
                ? 'Estimating…'
                : null;
  const summary = usage
    ? `${usage.estimated || usage.incomplete ? '≈ ' : ''}${count(usage.used)} tokens`
    : 'Usage unavailable';
  return (
    <div
      role="group"
      aria-label="Context usage"
      className="context-usage"
      onKeyDown={(event) => {
        if (event.key === 'Escape' && expanded) {
          event.stopPropagation();
          setExpanded(false);
          button.current?.focus();
        }
      }}
    >
      <button
        ref={button}
        type="button"
        className="context-usage-toggle"
        aria-expanded={expanded}
        aria-controls={identifier}
        onClick={() => setExpanded((value) => !value)}
      >
        <span>Context</span>
        <span className="context-usage-track" aria-hidden="true">
          <span
            style={{
              transform: `scaleX(${percentage === null ? 0 : Math.min(1, percentage / 100)})`,
            }}
          />
        </span>
        <span>
          {percentage === null ? summary : `${usage?.estimated ? '≈ ' : ''}${percentage}%`}
        </span>
        {status && <span className="context-usage-state">{status}</span>}
        <span aria-hidden="true">{expanded ? '−' : '+'}</span>
      </button>
      {expanded && (
        <section
          id={identifier}
          aria-label="Context usage details"
          className="context-usage-details"
        >
          <div className="context-usage-heading">
            <strong>{summary}</strong>
            <span>{usage?.model}</span>
          </div>
          {usage ? (
            <>
              <p>
                {usage.incomplete
                  ? 'Estimate incomplete: some input costs are unknown.'
                  : usage.estimated
                    ? 'Token counts are estimates.'
                    : 'Input total measured; category counts may be estimated.'}
              </p>
              {usage.reason && <p className="context-usage-reason">{usage.reason}</p>}
              <dl className="context-usage-breakdown">
                {(
                  Object.entries(usage.breakdown) as [keyof Usage['breakdown'], number | null][]
                ).map(([key, value]) => (
                  <div key={key}>
                    <dt>{labels[key]}</dt>
                    <dd>{count(value)}</dd>
                  </div>
                ))}
              </dl>
              <dl className="context-usage-budget">
                <div>
                  <dt>Total capacity</dt>
                  <dd>{count(usage.limit)}</dd>
                </div>
                <div>
                  <dt>Reserved for response</dt>
                  <dd>{count(usage.reserved)}</dd>
                </div>
                <div>
                  <dt>Auto-compaction at</dt>
                  <dd>{count(usage.threshold)}</dd>
                </div>
                <div>
                  <dt>Input before compaction</dt>
                  <dd>{count(usage.remaining)}</dd>
                </div>
              </dl>
              <p>
                Auto-compaction summarizes earlier history to make room. Original messages stay
                available. The threshold includes space for the response and safety headroom.
              </p>
              <p>
                Capacity source: {usage.source}.{' '}
                {usage.compacted_messages > 0
                  ? `${count(usage.compacted_messages)} messages summarized.`
                  : 'No messages summarized yet.'}
              </p>
            </>
          ) : (
            <p>
              {error ??
                (previewing
                  ? 'Estimating context for this draft…'
                  : 'This server has not supplied context usage. You can still send messages.')}
            </p>
          )}
          {usage && previewing && <p>Updating for your draft…</p>}
        </section>
      )}
    </div>
  );
}
