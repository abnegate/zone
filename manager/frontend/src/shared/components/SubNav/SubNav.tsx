import {
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react';
import './SubNav.css';

type SubNavProps = {
  children: ReactNode;
  className?: string;
  storageKey: string;
  label: string;
  defaultWidthRem?: number;
};

const DEFAULT_REM = 18;
const MIN_REM = 12;
const MAX_REM = 40;
const STEP_PX = 16;
const LARGE_STEP_PX = 32;

function rootPixels(): number {
  const size = parseFloat(getComputedStyle(document.documentElement).fontSize);
  return Number.isFinite(size) && size > 0 ? size : 16;
}

function remFromToken(name: string, fallback: number): number {
  const raw = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const match = /^(-?[\d.]+)rem$/.exec(raw);
  return match ? Number(match[1]) : fallback;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function metrics(defaultWidthRem?: number): { min: number; max: number; fallback: number } {
  const root = rootPixels();
  return {
    min: remFromToken('--ui-subnav-min', MIN_REM) * root,
    max: remFromToken('--ui-subnav-max', MAX_REM) * root,
    fallback: (defaultWidthRem ?? remFromToken('--ui-subnav-width', DEFAULT_REM)) * root,
  };
}

function ceiling(tokenMax: number, pane: HTMLElement | null): number {
  const parentWidth = pane?.parentElement?.getBoundingClientRect().width ?? 0;
  if (parentWidth <= 0) return tokenMax;
  return Math.min(tokenMax, Math.floor(parentWidth * 0.7));
}

function readWidth(storageKey: string, fallback: number, min: number, max: number): number {
  const raw = localStorage.getItem(storageKey);
  if (raw === null) return fallback;
  const parsed = Number(raw);
  if (!Number.isFinite(parsed)) return fallback;
  return clamp(Math.round(parsed), min, max);
}

export default function SubNav({
  children,
  className = '',
  storageKey,
  label,
  defaultWidthRem,
}: SubNavProps) {
  const paneRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const bounds = metrics(defaultWidthRem);
  const [width, setWidth] = useState(() =>
    readWidth(storageKey, bounds.fallback, bounds.min, bounds.max)
  );
  const [resizing, setResizing] = useState(false);
  const widthRef = useRef(width);

  useEffect(() => {
    widthRef.current = width;
  }, [width]);

  const persist = useCallback(
    (next: number) => {
      const max = ceiling(bounds.max, paneRef.current);
      const value = clamp(Math.round(next), bounds.min, max);
      setWidth(value);
      widthRef.current = value;
      localStorage.setItem(storageKey, String(value));
    },
    [bounds.max, bounds.min, storageKey]
  );

  const onPointerDown = (event: PointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    dragRef.current = { startX: event.clientX, startWidth: widthRef.current };
    event.currentTarget.setPointerCapture(event.pointerId);
    setResizing(true);
    document.documentElement.classList.add('sub-nav-resizing');
  };

  useEffect(() => {
    if (!resizing) return;

    const onMove = (event: globalThis.PointerEvent) => {
      const drag = dragRef.current;
      if (!drag) return;
      const max = ceiling(bounds.max, paneRef.current);
      const next = clamp(
        Math.round(drag.startWidth + (event.clientX - drag.startX)),
        bounds.min,
        max
      );
      setWidth(next);
      widthRef.current = next;
    };

    const onUp = () => {
      dragRef.current = null;
      setResizing(false);
      document.documentElement.classList.remove('sub-nav-resizing');
      localStorage.setItem(storageKey, String(Math.round(widthRef.current)));
    };

    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointercancel', onUp);
    return () => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      window.removeEventListener('pointercancel', onUp);
      document.documentElement.classList.remove('sub-nav-resizing');
    };
  }, [bounds.max, bounds.min, resizing, storageKey]);

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    const step = event.shiftKey ? LARGE_STEP_PX : STEP_PX;
    if (event.key === 'ArrowLeft') {
      event.preventDefault();
      persist(width - step);
    } else if (event.key === 'ArrowRight') {
      event.preventDefault();
      persist(width + step);
    } else if (event.key === 'Home') {
      event.preventDefault();
      persist(bounds.min);
    } else if (event.key === 'End') {
      event.preventDefault();
      persist(ceiling(bounds.max, paneRef.current));
    }
  };

  return (
    <div
      ref={paneRef}
      className={`sub-nav ${className} ${resizing ? 'resizing' : ''}`.trim()}
      style={{ '--sub-nav-width': `${width}px` } as CSSProperties}
    >
      {children}
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label={label}
        aria-valuemin={Math.round(bounds.min)}
        aria-valuemax={Math.round(ceiling(bounds.max, paneRef.current))}
        aria-valuenow={Math.round(width)}
        tabIndex={0}
        className="sub-nav-resize"
        data-testid="sub-nav-resize"
        onPointerDown={onPointerDown}
        onKeyDown={onKeyDown}
        onDoubleClick={() => persist(bounds.fallback)}
      />
    </div>
  );
}
