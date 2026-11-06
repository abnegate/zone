import { Badge } from '@zone/ui';
import { useCallback, useEffect, useId, useRef, useState } from 'react';
import { STEPS_LOAD_FAILED } from '../subagentSchemas';
import type { ChatSubagent, SubagentStep } from '../subagentTypes';
import {
  formatDuration,
  stepStatusLabel,
  subagentActive,
  subagentDuration,
  subagentName,
  subagentStatusLabel,
  subagentStatusTint,
} from '../utils/subagents';
import { SubagentSummary } from './SubagentSummary';
import './SubagentPulse.css';

export interface SubagentItemProps {
  subagent: ChatSubagent;
  /** The agent's steps once read; nothing until they have been. */
  steps: SubagentStep[] | undefined;
  onLoadSteps: (id: string) => Promise<void>;
  /** Expands the item, scrolls to it and focuses it whenever it changes to anything but zero. */
  reveal?: number;
}

const TICK = 1_000;
const TITLE = '.chat-subagent-title';

function useNow(ticking: boolean): number {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!ticking) return;
    setNow(Date.now());
    const interval = window.setInterval(() => setNow(Date.now()), TICK);
    return () => window.clearInterval(interval);
  }, [ticking]);

  return now;
}

function toolCount(count: number): string {
  return `${count} ${count === 1 ? 'tool' : 'tools'}`;
}

export function SubagentItem({ subagent, steps, onLoadSteps, reveal = 0 }: SubagentItemProps) {
  const [expanded, setExpanded] = useState(false);
  const [stepsError, setStepsError] = useState<string | null>(null);
  const row = useRef<HTMLLIElement>(null);
  const handled = useRef(reveal);
  const region = useId();
  const active = subagentActive(subagent);
  const now = useNow(active);
  const name = subagentName(subagent);
  const expandable = subagent.step_count > 0;
  const wanted = expanded && expandable && steps === undefined;
  const duration = subagentDuration(subagent, now);
  const described =
    Boolean(subagent.kind || subagent.model || subagent.last_tool) ||
    subagent.tool_uses !== null ||
    duration !== null;

  const requestSteps = useCallback(async () => {
    setStepsError(null);
    try {
      await onLoadSteps(subagent.id);
    } catch (failure) {
      setStepsError(failure instanceof Error ? failure.message : STEPS_LOAD_FAILED);
    }
  }, [onLoadSteps, subagent.id]);

  useEffect(() => {
    if (wanted) void requestSteps();
  }, [wanted, requestSteps]);

  useEffect(() => {
    if (reveal === handled.current) return;
    handled.current = reveal;
    if (!reveal) return;
    setExpanded(true);
    const frame = requestAnimationFrame(() => {
      row.current?.scrollIntoView?.({ block: 'nearest' });
      row.current?.querySelector<HTMLElement>(TITLE)?.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [reveal]);

  return (
    <li ref={row} className="chat-subagent" data-subagent={subagent.id}>
      <div className="chat-subagent-heading">
        {expandable ? (
          <button
            type="button"
            className="chat-subagent-title"
            aria-expanded={expanded}
            aria-controls={region}
            title={name}
            onClick={() => setExpanded((current) => !current)}
          >
            <svg
              className="chat-subagent-caret"
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
            <span className="chat-subagent-name">{name}</span>
          </button>
        ) : (
          <span className="chat-subagent-title" tabIndex={-1} title={name}>
            <span className="chat-subagent-name">{name}</span>
          </span>
        )}
        <Badge className="chat-subagent-status" variant={subagentStatusTint(subagent.status)}>
          {active && <span className="chat-subagent-pulse" aria-hidden="true" />}
          {subagentStatusLabel(subagent.status)}
        </Badge>
      </div>
      {described && (
        <div className="chat-subagent-meta">
          {subagent.kind && <span>{subagent.kind}</span>}
          {subagent.model && <span>{subagent.model}</span>}
          {subagent.tool_uses !== null && <span>{toolCount(subagent.tool_uses)}</span>}
          {duration !== null && (
            <span className="chat-subagent-duration">{formatDuration(duration)}</span>
          )}
          {subagent.last_tool && <span className="chat-subagent-tool">{subagent.last_tool}</span>}
        </div>
      )}
      {subagent.error ? (
        <p className="chat-subagent-error">{subagent.error}</p>
      ) : (
        subagent.summary && <SubagentSummary key={subagent.summary} text={subagent.summary} />
      )}
      {expandable && (
        <div id={region} className="chat-subagent-steps-region" hidden={!expanded}>
          {steps ? (
            <ol className="chat-subagent-steps">
              {steps.map((step) => (
                <li
                  key={step.id}
                  className={`chat-subagent-step chat-subagent-step--${step.status}`}
                >
                  <span className="chat-subagent-step-status" aria-hidden="true" />
                  <span className="sr-only">{stepStatusLabel(step.status)}</span>
                  <span className="chat-subagent-step-name">{step.name}</span>
                  {step.subject && (
                    <code className="chat-subagent-step-subject">{step.subject}</code>
                  )}
                </li>
              ))}
            </ol>
          ) : stepsError ? (
            <p className="chat-subagent-steps-note chat-subagent-steps-error" role="alert">
              {stepsError}
              <button
                type="button"
                className="chat-subagent-steps-retry"
                onClick={() => void requestSteps()}
              >
                Retry
              </button>
            </p>
          ) : (
            <p className="chat-subagent-steps-note" role="status">
              Loading steps...
            </p>
          )}
        </div>
      )}
    </li>
  );
}
