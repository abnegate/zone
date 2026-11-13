import { Button } from '@zone/ui';
import { useEffect, useRef } from 'react';

export const LOAD_MORE_CHATS = 'Load more chats';
export const LOADING_MORE_CHATS = 'Loading more chats…';

interface MoreChatsProps {
  remaining: number;
  loading: boolean;
  onLoad: () => void;
}

/** The end of a paged chat list: loads the next page when scrolled near, or when clicked. */
export function MoreChats({ remaining, loading, onLoad }: MoreChatsProps) {
  const sentinel = useRef<HTMLDivElement>(null);
  const load = useRef(onLoad);
  load.current = onLoad;

  useEffect(() => {
    const element = sentinel.current;
    if (loading || !element || typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) load.current();
      },
      { rootMargin: '200px 0px' }
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [loading]);

  return (
    <div ref={sentinel} className="chats-more">
      <Button variant="ghost" size="sm" onClick={onLoad} disabled={loading}>
        {loading ? LOADING_MORE_CHATS : `${LOAD_MORE_CHATS} (${remaining})`}
      </Button>
    </div>
  );
}
