import { type RefObject, useCallback, useEffect, useLayoutEffect, useRef } from 'react';
import { controlIn, focusLost, HANDLE_SELECTOR } from './focus';

export function useFollowedCard(root: RefObject<HTMLElement | null>): (id: string) => void {
  const followed = useRef<string | null>(null);

  useLayoutEffect(() => {
    const id = followed.current;
    if (id === null || !focusLost()) return;
    controlIn(root.current, id, HANDLE_SELECTOR)?.focus();
  });

  useEffect(() => {
    const release = (event: Event) => {
      const id = followed.current;
      if (id === null) return;
      if (
        event.type === 'focusin' &&
        event.target === controlIn(root.current, id, HANDLE_SELECTOR)
      ) {
        return;
      }
      followed.current = null;
    };
    document.addEventListener('focusin', release);
    document.addEventListener('pointerdown', release, true);
    return () => {
      document.removeEventListener('focusin', release);
      document.removeEventListener('pointerdown', release, true);
    };
  }, [root]);

  return useCallback((id: string) => {
    followed.current = id;
  }, []);
}
