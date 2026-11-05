import { type RefObject, useEffect } from 'react';

const TABBABLE = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

function tabbables(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(TABBABLE)).filter(
    (element) =>
      element.tabIndex >= 0 && !element.closest('[inert]') && element.getClientRects().length > 0
  );
}

/**
 * While active, moves focus into the container, keeps Tab and Shift+Tab cycling
 * inside it, and hands focus back to whatever held it before once released.
 */
export function useFocusTrap(container: RefObject<HTMLElement | null>, active: boolean): void {
  useEffect(() => {
    const element = container.current;
    if (!active || !element) return undefined;

    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    element.focus();

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Tab') return;
      const candidates = tabbables(element);
      const first = candidates[0];
      const last = candidates[candidates.length - 1];
      if (!first || !last) {
        event.preventDefault();
        return;
      }
      const current = document.activeElement;
      const outside = !(current instanceof Node) || !element.contains(current);
      if (event.shiftKey && (outside || current === first || current === element)) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (outside || current === last)) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener('keydown', handleKeyDown);
    return () => {
      document.removeEventListener('keydown', handleKeyDown);
      if (previous?.isConnected) previous.focus();
    };
  }, [container, active]);
}
