import { useId, useLayoutEffect, useRef, useState } from 'react';

export interface SubagentSummaryProps {
  text: string;
}

/**
 * An agent's report, clamped to a few lines with a toggle when it runs longer.
 * Give it a `key` of the text, so a new report is measured and starts clamped.
 */
export function SubagentSummary({ text }: SubagentSummaryProps) {
  const [open, setOpen] = useState(false);
  const [overflowing, setOverflowing] = useState(false);
  const body = useRef<HTMLParagraphElement>(null);
  const id = useId();

  useLayoutEffect(() => {
    const element = body.current;
    if (!element || open) return;
    const measure = () => setOverflowing(element.scrollHeight > element.clientHeight);
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [open]);

  return (
    <>
      <p ref={body} id={id} className="chat-subagent-summary" data-clamped={!open}>
        {text}
      </p>
      {(overflowing || open) && (
        <button
          type="button"
          className="chat-subagent-summary-toggle"
          aria-expanded={open}
          aria-controls={id}
          onClick={() => setOpen((current) => !current)}
        >
          {open ? 'Show less' : 'Show more'}
        </button>
      )}
    </>
  );
}
