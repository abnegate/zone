import { useEffect, useRef, useState } from 'react';
import { panelKey } from '../utils/panel';
import type { ChatSources, SourceRequest } from '../utils/sources';
import { CitationItem } from './CitationItem';
import { PanelBox } from './PanelBox';
import './SourcesBox.css';

export interface SourcesBoxProps {
  sources: ChatSources;
  userId: string | null;
  shown?: SourceRequest | null;
  onShown?: () => void;
}

interface Focus {
  key: string | null;
}

const HIGHLIGHT_DURATION = 2400;

export function SourcesBox({ sources, userId, shown = null, onShown }: SourcesBoxProps) {
  const list = useRef<HTMLOListElement>(null);
  const [reveal, setReveal] = useState(0);
  const [reply, setReply] = useState<string | null>(null);
  const [focus, setFocus] = useState<Focus | null>(null);
  const replied = reply ? sources.replies.get(reply) : undefined;
  const listed = replied ?? sources.all;

  useEffect(() => {
    if (!shown) return;
    setReveal((count) => count + 1);
    setReply(shown.messageId);
    setFocus({ key: shown.key });
    onShown?.();
  }, [shown, onShown]);

  useEffect(() => {
    if (!focus) return;
    const frame = requestAnimationFrame(() => {
      const rows = Array.from(list.current?.querySelectorAll<HTMLElement>('[data-source]') ?? []);
      const row = rows.find((candidate) => candidate.dataset.source === focus.key) ?? rows[0];
      row?.scrollIntoView?.({ block: 'nearest' });
      row?.querySelector<HTMLElement>('.citation-link')?.focus({ preventScroll: true });
    });
    const timer = setTimeout(() => setFocus(null), HIGHLIGHT_DURATION);
    return () => {
      cancelAnimationFrame(frame);
      clearTimeout(timer);
    };
  }, [focus]);

  return (
    <PanelBox
      title="Sources"
      count={listed.length}
      storageKey={panelKey(userId, 'sources')}
      className="chat-citations"
      reveal={reveal}
    >
      {replied && (
        <div className="chat-citations-scope">
          <span>Cited in this reply</span>
          <button
            type="button"
            className="chat-citations-all"
            onClick={() => {
              setReply(null);
              setFocus({ key: null });
            }}
          >
            All {sources.all.length} sources
          </button>
        </div>
      )}
      <ol ref={list} className="citation-list">
        {listed.map((source) => (
          <CitationItem
            key={source.key}
            citation={source.citation}
            sourceKey={source.key}
            highlighted={focus?.key === source.key}
          />
        ))}
      </ol>
    </PanelBox>
  );
}
