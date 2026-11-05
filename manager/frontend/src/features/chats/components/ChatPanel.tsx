import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@zone/ui';
import {
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from 'react';
import { useBoxOrder } from '../hooks/useBoxOrder';
import type { ChatPullRequests } from '../hooks/useChatPullRequests';
import type { ChatTasks } from '../hooks/useChatTasks';
import type { PanelBoxId } from '../utils/boxes';
import type { GalleryItem } from '../utils/gallery';
import {
  clampWidth,
  PANEL_WIDTH,
  panelKey,
  readFlag,
  readWidth,
  writeSetting,
} from '../utils/panel';
import type { ChatSources, SourceRequest } from '../utils/sources';
import { BoxList } from './BoxList';
import { ChangesBox } from './ChangesBox';
import { GalleryBox } from './GalleryBox';
import { PanelIcon } from './PanelIcon';
import { SourcesBox } from './SourcesBox';
import { TasksBox } from './TasksBox';
import './ChatPanel.css';

export interface ChatPanelProps {
  chatId: string;
  userId: string | null;
  changes: ChatPullRequests;
  tasks: ChatTasks;
  media: readonly GalleryItem[];
  sources: ChatSources;
  shownSources?: SourceRequest | null;
  onSourcesShown?: () => void;
  onShowMessage: (messageId: string) => void;
}

type Placement = 'panel' | 'sheet';

const PANEL_LABEL = 'Changes, tasks, media and sources';
const RESIZING = 'chat-panel-resizing';
/** The most of the chat's width the panel may take, leaving the rest to the transcript. */
const MAX_SHARE = 0.5;

function counted(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}

/** Below the phone breakpoint ChatPanel.css hides the panel, and the sheet stands in for it. */
function placement(panel: HTMLElement | null): Placement {
  return panel && getComputedStyle(panel).display === 'none' ? 'sheet' : 'panel';
}

/**
 * The chat's pull requests, tasks, media and sources, beside the transcript at the top right
 * of the chat. It can be resized, hidden to a rail, and on a phone opens as a
 * sheet instead, so it never sits over the conversation.
 */
export function ChatPanel({
  chatId,
  userId,
  changes,
  tasks,
  media,
  sources,
  shownSources = null,
  onSourcesShown,
  onShowMessage,
}: ChatPanelProps) {
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
  const rail = useRef<HTMLButtonElement>(null);
  const hide = useRef<HTMLButtonElement>(null);
  const toggled = useRef(false);
  const opener = useRef<HTMLElement | null>(null);
  const { order, move } = useBoxOrder(userId);
  const dragging = useRef(false);

  const openSheet = useCallback(() => {
    opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setSheet(true);
  }, []);

  function returnFocus(event: Event) {
    const target = opener.current;
    opener.current = null;
    if (!target?.isConnected) return;
    event.preventDefault();
    target.focus();
  }

  const showingIn = shownSources ? placement(panel.current) : null;

  useEffect(() => {
    if (!shownSources) return;
    if (placement(panel.current) === 'sheet') openSheet();
    else setCollapsed(false);
  }, [shownSources, openSheet]);

  useEffect(() => {
    if (!toggled.current) return;
    toggled.current = false;
    (collapsed ? rail : hide).current?.focus();
  }, [collapsed]);

  useEffect(() => {
    const stored = readWidth(widthKey);
    latest.current = stored;
    setWidth(stored);
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
  const showsTasks = tasks.tasks.length > 0 || tasks.error !== null;
  const showsGallery = media.length > 0;
  const showsSources = sources.all.length > 0;
  if (!showsChanges && !showsTasks && !showsGallery && !showsSources) return null;

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
    toggled.current = true;
    setCollapsed(hidden);
    writeSetting(collapsedKey, String(hidden));
  };

  const summary = [
    showsChanges ? counted(changes.pullRequests.length, 'change', 'changes') : null,
    showsTasks ? counted(tasks.tasks.length, 'task', 'tasks') : null,
    showsGallery ? counted(media.length, 'media item', 'media items') : null,
    showsSources ? counted(sources.all.length, 'source', 'sources') : null,
  ]
    .filter(Boolean)
    .join(', ');

  const boxes = (target: Placement) => {
    const rendered: Record<PanelBoxId, ReactNode> = {
      changes: showsChanges && <ChangesBox chatId={chatId} userId={userId} changes={changes} />,
      tasks: showsTasks && <TasksBox userId={userId} tasks={tasks} />,
      sources: showsSources && (
        <SourcesBox
          key={chatId}
          sources={sources}
          userId={userId}
          shown={showingIn === target ? shownSources : null}
          onShown={onSourcesShown}
        />
      ),
      gallery: showsGallery && (
        <GalleryBox
          items={media}
          userId={userId}
          onShowMessage={(messageId) => {
            opener.current = null;
            setSheet(false);
            onShowMessage(messageId);
          }}
        />
      ),
    };
    const listed = order.filter((id) => rendered[id]).map((id) => ({ id, node: rendered[id] }));
    const shown = listed.map((box) => box.id);
    return (
      <BoxList
        boxes={listed}
        onMove={(id, index) => move(shown, id, index)}
        onDraggingChange={(value) => {
          dragging.current = value;
        }}
      />
    );
  };

  return (
    <>
      <div className="chat-panel-toggle-row">
        <button
          type="button"
          className="chat-panel-toggle"
          aria-haspopup="dialog"
          aria-label={`${PANEL_LABEL}: ${summary}`}
          title={PANEL_LABEL}
          onClick={openSheet}
        >
          <PanelIcon />
          <span className="chat-panel-toggle-summary">{summary}</span>
        </button>
      </div>
      <Dialog open={sheet} onOpenChange={setSheet}>
        {sheet && (
          <DialogContent
            className="chat-panel-sheet"
            onCloseAutoFocus={returnFocus}
            onEscapeKeyDown={(event) => {
              if (dragging.current) event.preventDefault();
            }}
          >
            <DialogTitle className="chat-panel-sheet-title">{PANEL_LABEL}</DialogTitle>
            <DialogDescription className="sr-only">{summary}</DialogDescription>
            <div className="chat-panel-boxes">{boxes('sheet')}</div>
          </DialogContent>
        )}
      </Dialog>
      {collapsed ? (
        <aside ref={panel} className="chat-panel is-collapsed" aria-label={PANEL_LABEL}>
          <button
            type="button"
            ref={rail}
            className="chat-panel-rail"
            aria-expanded={false}
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
              ref={hide}
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
          <div className="chat-panel-boxes">{boxes('panel')}</div>
        </aside>
      )}
    </>
  );
}
