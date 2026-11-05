import { useEffect, useRef, useState } from 'react';
import { panelKey } from '../utils/panel';
import type { ChatSource, SourceRequest } from '../utils/sources';
import { CitationItem } from './CitationItem';
import { PanelBox } from './PanelBox';
import './SourcesBox.css';

export interface SourcesBoxProps {
  sources: readonly ChatSource[];
  userId: string | null;
  shown?: SourceRequest | null;
  onShown?: () => void;
}

const HIGHLIGHT_DURATION = 2400;
const NONE: ReadonlySet<string> = new Set();

/** The checkable sources behind the chat's replies, newest first. */
export function SourcesBox({ sources, userId, shown = null, onShown }: SourcesBoxProps) {
  const list = useRef<HTMLOListElement>(null);
  const [reveal, setReveal] = useState(0);
  const [highlighted, setHighlighted] = useState(NONE);

  useEffect(() => {
    if (!shown) return;
    setReveal((count) => count + 1);
    setHighlighted(new Set(shown.keys));
    onShown?.();
  }, [shown, onShown]);

  useEffect(() => {
    if (highlighted.size === 0) return;
    const frame = requestAnimationFrame(() => {
      const rows = list.current?.querySelectorAll<HTMLElement>('[data-source]') ?? [];
      const first = Array.from(rows).find((row) => highlighted.has(row.dataset.source ?? ''));
      first?.scrollIntoView?.({ block: 'nearest' });
      first?.querySelector<HTMLElement>('.citation-link')?.focus({ preventScroll: true });
    });
    const timer = setTimeout(() => setHighlighted(NONE), HIGHLIGHT_DURATION);
    return () => {
      cancelAnimationFrame(frame);
      clearTimeout(timer);
    };
  }, [highlighted]);

  return (
    <PanelBox
      title="Sources"
      count={sources.length}
      storageKey={panelKey(userId, 'sources')}
      className="chat-sources"
      reveal={reveal}
    >
      <ol ref={list} className="citation-list">
        {sources.map((source) => (
          <CitationItem
            key={source.key}
            citation={source.citation}
            sourceKey={source.key}
            highlighted={highlighted.has(source.key)}
          />
        ))}
      </ol>
    </PanelBox>
  );
}
