import { type RefObject, useEffect, useState } from 'react';

/**
 * Whether the element has come within a screen of being seen. Once it has it
 * stays so, so what it loaded is kept. Without IntersectionObserver every
 * element counts as seen.
 */
export function useVisible(ref: RefObject<Element | null>): boolean {
  const [visible, setVisible] = useState(() => typeof IntersectionObserver === 'undefined');

  useEffect(() => {
    const element = ref.current;
    if (visible || !element) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setVisible(true);
          observer.disconnect();
        }
      },
      { rootMargin: '100% 0px' }
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref, visible]);

  return visible;
}
