import { type ReactNode, useEffect, useId, useRef, useState } from 'react';

const SELECT_MENU = '[data-radix-popper-content-wrapper]';
const OPEN_SELECT = '[role="combobox"][aria-expanded="true"]';

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
    const selecting = () => Boolean(root.current?.querySelector(OPEN_SELECT));
    // Captured because a select trigger elsewhere cancels the press it opens on.
    const close = (event: PointerEvent) => {
      if (selecting() || root.current?.contains(event.target as Node)) return;
      setOpen(false);
    };
    // Bubbling so a control inside the panel sees the Escape before focus leaves it.
    const dismiss = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || selecting()) return;
      if ((event.target as Element).closest?.(SELECT_MENU)) return;
      setOpen(false);
      button.current?.focus();
    };
    const leave = (event: FocusEvent) => {
      const next = event.relatedTarget as Element | null;
      if (!next || root.current?.contains(next) || next.closest(SELECT_MENU)) return;
      setOpen(false);
    };
    const element = root.current;
    document.addEventListener('pointerdown', close, true);
    document.addEventListener('keydown', dismiss);
    element?.addEventListener('focusout', leave);
    return () => {
      document.removeEventListener('pointerdown', close, true);
      document.removeEventListener('keydown', dismiss);
      element?.removeEventListener('focusout', leave);
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
