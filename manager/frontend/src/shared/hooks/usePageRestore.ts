import { useEffect, useEffectEvent } from 'react';

export function usePageRestore(onRestore: () => void): void {
  const restore = useEffectEvent(onRestore);

  useEffect(() => {
    const listener = (event: PageTransitionEvent) => {
      if (event.persisted) {
        restore();
      }
    };
    window.addEventListener('pageshow', listener);
    return () => window.removeEventListener('pageshow', listener);
  }, []);
}
