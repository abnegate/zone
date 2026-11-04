import * as MenuPrimitive from '@radix-ui/react-dropdown-menu';
import { type KeyboardEvent, type ReactNode, type SyntheticEvent, useRef } from 'react';
import { cn } from '../../lib/utils';
import './ActionMenu.css';

export interface ActionMenuItem {
  label: string;
  onSelect: () => void;
  icon?: ReactNode;
  destructive?: boolean;
}

export interface ActionMenuProps {
  label: string;
  items: readonly ActionMenuItem[];
}

const OPENING_KEYS: ReadonlySet<string> = new Set(['Enter', ' ', 'ArrowDown']);

function isolate(event: SyntheticEvent): void {
  event.stopPropagation();
}

function isolateOpening(event: KeyboardEvent): void {
  if (OPENING_KEYS.has(event.key)) event.stopPropagation();
}

function MoreIcon() {
  return (
    <svg className="ui-action-menu-icon" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <circle cx="5" cy="12" r="1.75" />
      <circle cx="12" cy="12" r="1.75" />
      <circle cx="19" cy="12" r="1.75" />
    </svg>
  );
}

/**
 * A trigger that opens a menu of actions for one thing, such as a row in a
 * list. Clicks and key presses that work the menu stay inside it, so a
 * clickable row around it never sees them. A chosen action runs only once
 * focus is back on the trigger, so the action is free to move focus itself,
 * such as into a dialog it opens.
 */
export function ActionMenu({ label, items }: ActionMenuProps) {
  const trigger = useRef<HTMLButtonElement>(null);
  const chosen = useRef<ActionMenuItem | null>(null);

  const restoreFocus = (event: Event) => {
    const item = chosen.current;
    chosen.current = null;
    if (!item) return;
    event.preventDefault();
    trigger.current?.focus();
    item.onSelect();
  };

  return (
    <MenuPrimitive.Root>
      <MenuPrimitive.Trigger
        ref={trigger}
        className="ui-action-menu-trigger"
        aria-label={label}
        title={label}
        onClick={isolate}
        onKeyDown={isolateOpening}
      >
        <MoreIcon />
      </MenuPrimitive.Trigger>
      <MenuPrimitive.Portal>
        <MenuPrimitive.Content
          className="ui-action-menu-content"
          align="end"
          sideOffset={4}
          collisionPadding={8}
          onCloseAutoFocus={restoreFocus}
          onClick={isolate}
          onKeyDown={isolate}
        >
          {items.map((item) => (
            <MenuPrimitive.Item
              key={item.label}
              className={cn(
                'ui-action-menu-item',
                item.destructive && 'ui-action-menu-item-destructive'
              )}
              onSelect={() => {
                chosen.current = item;
              }}
            >
              {item.icon && (
                <span className="ui-action-menu-item-icon" aria-hidden="true">
                  {item.icon}
                </span>
              )}
              {item.label}
            </MenuPrimitive.Item>
          ))}
        </MenuPrimitive.Content>
      </MenuPrimitive.Portal>
    </MenuPrimitive.Root>
  );
}
