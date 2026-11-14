import { useDroppable } from '@dnd-kit/core';
import { type ReactNode, useCallback, useId, useLayoutEffect, useRef } from 'react';
import type { Stage } from '../../types';
import { StatusIcon } from './StatusIcon';
import type { View } from './View';
import './Column.css';

interface ColumnProps {
  stage: Stage;
  count: number;
  view: View;
  children: ReactNode;
}

export function Column({ stage, count, view, children }: ColumnProps) {
  const heading = useId();
  const { isOver, setNodeRef } = useDroppable({ id: stage.id, data: { stage } });
  const body = useRef<HTMLDivElement | null>(null);
  const attach = useCallback(
    (node: HTMLDivElement | null) => {
      body.current = node;
      setNodeRef(node);
    },
    [setNodeRef]
  );

  useLayoutEffect(() => {
    const scroller = body.current;
    if (!scroller) return undefined;
    scroller.scrollTop = view.tops.get(stage.id) ?? 0;
    return () => {
      view.tops.set(stage.id, scroller.scrollTop);
    };
  }, [view, stage.id]);

  return (
    <section className="board-column" data-stage={stage.id} aria-labelledby={heading} tabIndex={-1}>
      <header className="board-column-header">
        <StatusIcon stage={stage.id} />
        <h2 id={heading}>{stage.label}</h2>
        <span className="board-column-count">{count}</span>
      </header>
      <div ref={attach} className="board-column-body" data-over={isOver || undefined}>
        <ul className="board-column-cards">{children}</ul>
        {count === 0 && <p className="board-column-empty">No tasks</p>}
      </div>
    </section>
  );
}
