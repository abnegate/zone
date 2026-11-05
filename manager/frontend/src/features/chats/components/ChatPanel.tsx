import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@zone/ui';
import {
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from 'react';
import type { ChatPullRequests } from '../hooks/useChatPullRequests';
import type { GalleryItem } from '../utils/gallery';
import {
  clampWidth,
  PANEL_WIDTH,
  panelKey,
  readFlag,
  readWidth,
  writeSetting,
} from '../utils/panel';
import { ChangesBox } from './ChangesBox';
import { GalleryBox } from './GalleryBox';
import './ChatPanel.css';

export interface ChatPanelProps {
  chatId: string;
  userId: string | null;
  changes: ChatPullRequests;
  media: readonly GalleryItem[];
  onShowMessage: (messageId: string) => void;
}

const PANEL_LABEL = 'Changes and media';
const RESIZING = 'chat-panel-resizing';
/** The most of the chat's width the panel may take, leaving the rest to the transcript. */
const MAX_SHARE = 0.5;

function PanelIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M15 4v16" />
    </svg>
  );
}

/**
 * The chat's pull requests and media, beside the transcript at the top right
 * of the chat. It can be resized, hidden to a rail, and on a phone opens as a
 * sheet instead, so it never sits over the conversation.
 */
export function ChatPanel({ chatId, userId, changes, media, onShowMessage }: ChatPanelProps) {
  const widthKey = panelKey(userId, 'width');
  const collapsedKey = panelKey(userId, 'collapsed');
  const [width, setWidth] = useState(() => readWidth(widthKey));
  const [collapsed, setCollapsed] = useState(() => readFlag(collapsedKey));
  const [sheet, setSheet] = useState(false);
  const [resizing, setResizing] = useState(false);
  const panel = useRef<HTMLElement>(null);
  const drag = useRef<{ startX: number; startWidth: number } | null>(null);
  const latest = useRef(width);
  const panelId = useId();

  useEffect(() => {
    setWidth(readWidth(widthKey));
    setCollapsed(readFlag(collapsedKey));
  }, [widthKey, collapsedKey]);

  const ceiling = useCallback(() => {
    const available = panel.current?.parentElement?.getBoundingClientRect().width ?? 0;
    return available > 0 ? Math.floor(available * MAX_SHARE) : PANEL_WIDTH.max;
  }, []);

  const resize = useCallback(
    (next: number, persist: boolean) => {
      const value = clampWidth(next, ceiling());
      latest.current = value;
      setWidth(value);
      if (persist) writeSetting(widthKey, String(value));
    },
    [ceiling, widthKey]
  );

  useEffect(() => {
    if (!resizing) return;
    const move = (event: globalThis.PointerEvent) => {
      const start = drag.current;
      if (start) resize(start.startWidth - (event.clientX - start.startX), false);
    };
    const end = () => {
      drag.current = null;
      setResizing(false);
      writeSetting(widthKey, String(latest.current));
    };
    document.documentElement.classList.add(RESIZING);
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', end);
    window.addEventListener('pointercancel', end);
    return () => {
      document.documentElement.classList.remove(RESIZING);
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', end);
      window.removeEventListener('pointercancel', end);
    };
  }, [resizing, resize, widthKey]);

  const showsChanges = changes.pullRequests.length > 0 || changes.dismissed !== null;
  const showsGallery = media.length > 0;
  if (!showsChanges && !showsGallery) return null;

  const startDrag = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    drag.current = { startX: event.clientX, startWidth: latest.current };
    event.currentTarget.setPointerCapture?.(event.pointerId);
    setResizing(true);
  };

  const onResizeKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.shiftKey ? PANEL_WIDTH.largeStep : PANEL_WIDTH.step;
    const target: Record<string, number> = {
      ArrowLeft: width + step,
      ArrowRight: width - step,
      Home: PANEL_WIDTH.min,
      End: ceiling(),
    };
    if (!(event.key in target)) return;
    event.preventDefault();
    resize(target[event.key], true);
  };

  const setHidden = (hidden: boolean) => {
    setCollapsed(hidden);
    writeSetting(collapsedKey, String(hidden));
  };

  const summary = [
    showsChanges ? `${changes.pullRequests.length} changes` : null,
    showsGallery ? `${media.length} media` : null,
  ]
    .filter(Boolean)
    .join(', ');

  const boxes = (
    <>
      {showsChanges && <ChangesBox chatId={chatId} userId={userId} changes={changes} />}
      {showsGallery && (
        <GalleryBox
          items={media}
          userId={userId}
          onShowMessage={(messageId) => {
            setSheet(false);
            onShowMessage(messageId);
          }}
        />
      )}
    </>
  );

  return (
    <>
      <div className="chat-panel-toggle-row">
        <button
          type="button"
          className="chat-panel-toggle"
          aria-haspopup="dialog"
          onClick={() => setSheet(true)}
        >
          <PanelIcon />
          <span>{PANEL_LABEL}</span>
          <span className="chat-panel-toggle-summary">{summary}</span>
        </button>
      </div>
      <Dialog open={sheet} onOpenChange={setSheet}>
        {sheet && (
          <DialogContent className="chat-panel-sheet">
            <DialogTitle className="chat-panel-sheet-title">{PANEL_LABEL}</DialogTitle>
            <DialogDescription className="sr-only">{summary}</DialogDescription>
            <div className="chat-panel-boxes">{boxes}</div>
          </DialogContent>
        )}
      </Dialog>
      {collapsed ? (
        <aside ref={panel} className="chat-panel is-collapsed" aria-label={PANEL_LABEL}>
          <button
            type="button"
            className="chat-panel-rail"
            aria-expanded={false}
            aria-controls={panelId}
            aria-label={`Show ${PANEL_LABEL.toLowerCase()} (${summary})`}
            title={`Show ${PANEL_LABEL.toLowerCase()}`}
            onClick={() => setHidden(false)}
          >
            <PanelIcon />
          </button>
        </aside>
      ) : (
        <aside
          ref={panel}
          id={panelId}
          className={`chat-panel${resizing ? ' is-resizing' : ''}`}
          aria-label={PANEL_LABEL}
          style={{ '--chat-panel-width': `${width}px` } as CSSProperties}
        >
          <div
            role="separator"
            tabIndex={0}
            className="chat-panel-resize"
            aria-orientation="vertical"
            aria-label={`Resize ${PANEL_LABEL.toLowerCase()}`}
            aria-controls={panelId}
            aria-valuemin={PANEL_WIDTH.min}
            aria-valuemax={Math.max(PANEL_WIDTH.min, Math.min(PANEL_WIDTH.max, ceiling()))}
            aria-valuenow={width}
            data-testid="chat-panel-resize"
            onPointerDown={startDrag}
            onKeyDown={onResizeKey}
            onDoubleClick={() => resize(PANEL_WIDTH.fallback, true)}
          />
          <div className="chat-panel-bar">
            <span className="chat-panel-bar-title">{PANEL_LABEL}</span>
            <button
              type="button"
              className="chat-panel-hide"
              aria-expanded={true}
              aria-controls={panelId}
              aria-label={`Hide ${PANEL_LABEL.toLowerCase()}`}
              title={`Hide ${PANEL_LABEL.toLowerCase()}`}
              onClick={() => setHidden(true)}
            >
              <PanelIcon />
            </button>
          </div>
          <div className="chat-panel-boxes">{boxes}</div>
        </aside>
      )}
    </>
  );
}
