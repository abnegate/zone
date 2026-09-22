import { useState } from 'react';
import type { ToolCallRecord } from '../types';
import { Reasoning } from './Reasoning';
import { ToolTrace } from './ToolTrace';

const LABEL = 'Reasoning';

function hasThinking(call: ToolCallRecord): boolean {
  return Boolean(call.reasoning?.trim());
}

function needsAttention(call: ToolCallRecord): boolean {
  return call.approval === 'pending' || Boolean(call.pending);
}

/**
 * Everything an assistant turn did before it answered: its thinking and the
 * tools it called, in the order they happened, under one label. The thinking
 * folds away as a whole once the turn is over; the tool rows stay.
 */
export function Activity({
  reasoning,
  calls,
  live,
  answered,
  onDecide,
  onAnswer,
}: {
  reasoning?: string;
  calls: ToolCallRecord[];
  live: boolean;
  answered: boolean;
  onDecide?: (id: string, approved: boolean) => void;
  onAnswer?: (content: string) => void;
}) {
  const [expanded, setExpanded] = useState(live || calls.some(needsAttention));
  const leftover = reasoning?.trim() ?? '';
  const traced = calls.some(hasThinking);
  const thinking = traced || leftover.length > 0;

  if (!thinking && calls.length === 0) return null;

  return (
    <section className="message-activity" data-testid="activity">
      {thinking ? (
        <button
          type="button"
          className="message-activity-toggle"
          aria-expanded={expanded}
          onClick={() => setExpanded((current) => !current)}
        >
          <svg
            className="message-activity-caret"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <path d="m9 6 6 6-6 6" />
          </svg>
          <span>{LABEL}</span>
        </button>
      ) : null}
      {!traced && leftover ? <Reasoning content={leftover} open={expanded} /> : null}
      {calls.length > 0 ? (
        <ToolTrace
          calls={calls}
          thinking={expanded}
          answered={answered}
          live={live}
          onDecide={onDecide}
          onAnswer={onAnswer}
        />
      ) : null}
      {traced && leftover ? <Reasoning content={leftover} open={expanded} /> : null}
    </section>
  );
}
