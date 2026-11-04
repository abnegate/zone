import { type ReactNode, useEffect, useId, useRef, useState } from 'react';

const SELECT_MENU = '[data-radix-popper-content-wrapper]';

export interface HeaderPopoverProps {
  className: string;
  testId: string;
  label: string;
  triggerLabel: string;
  title: string;
  disabled?: boolean;
  trigger: ReactNode;
  children: ReactNode;
}

/**
 * A pill in the chat header that opens a panel of further controls. The panel
 * stays mounted while closed so a half-made choice inside it survives closing.
 */
export function HeaderPopover({
  className,
  testId,
  label,
  triggerLabel,
  title,
  disabled = false,
  trigger,
  children,
}: HeaderPopoverProps) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const panelId = useId();

  useEffect(() => {
    if (!open) return;
    panel.current?.focus();
    const close = (event: MouseEvent) => {
      if (document.querySelector(SELECT_MENU)) return;
      if (root.current?.contains(event.target as Node)) return;
      setOpen(false);
    };
    // Captured so it runs before an open select closes itself on the same Escape.
    const dismiss = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || document.querySelector(SELECT_MENU)) return;
      setOpen(false);
      button.current?.focus();
    };
    document.addEventListener('mousedown', close);
    document.addEventListener('keydown', dismiss, true);
    return () => {
      document.removeEventListener('mousedown', close);
      document.removeEventListener('keydown', dismiss, true);
    };
  }, [open]);

  return (
    <div className="chat-header-popover" ref={root}>
      <button
        type="button"
        ref={button}
        className={className}
        onClick={() => setOpen((current) => !current)}
        aria-expanded={open}
        aria-controls={panelId}
        aria-haspopup="dialog"
        aria-label={triggerLabel}
        title={title}
        disabled={disabled}
        data-testid={testId}
      >
        {trigger}
        <svg
          className="chat-header-popover-chevron"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          aria-hidden="true"
        >
          <path d="m6 9 6 6 6-6" />
        </svg>
      </button>
      <div
        className="chat-header-popover-panel"
        id={panelId}
        ref={panel}
        role="dialog"
        aria-label={label}
        tabIndex={-1}
        hidden={!open}
      >
        {children}
      </div>
    </div>
  );
}
