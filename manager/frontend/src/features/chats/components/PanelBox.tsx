import { type ReactNode, useId, useState } from 'react';
import { readFlag, writeSetting } from '../utils/panel';

export interface PanelBoxProps {
  title: string;
  count: number;
  /** Where whether the box is collapsed is remembered. */
  storageKey: string;
  className?: string;
  children: ReactNode;
}

/** One collapsible box of the chat panel, which remembers whether it was open. */
export function PanelBox({ title, count, storageKey, className, children }: PanelBoxProps) {
  const [collapsed, setCollapsed] = useState(() => readFlag(storageKey));
  const heading = useId();
  const body = useId();

  const toggle = () => {
    const next = !collapsed;
    setCollapsed(next);
    writeSetting(storageKey, String(next));
  };

  return (
    <section
      className={['chat-panel-box', className].filter(Boolean).join(' ')}
      aria-labelledby={heading}
    >
      <h4 className="chat-panel-box-heading">
        <button
          type="button"
          id={heading}
          className="chat-panel-box-toggle"
          aria-expanded={!collapsed}
          aria-controls={body}
          aria-label={`${title}, ${count}`}
          onClick={toggle}
        >
          <svg
            className="chat-panel-box-chevron"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            aria-hidden="true"
          >
            <path d="m9 6 6 6-6 6" />
          </svg>
          <span className="chat-panel-box-title">{title}</span>
          <span className="chat-panel-box-count">{count}</span>
        </button>
      </h4>
      <div id={body} className="chat-panel-box-body" hidden={collapsed}>
        {children}
      </div>
    </section>
  );
}
