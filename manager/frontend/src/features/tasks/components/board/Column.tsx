import { useDroppable } from '@dnd-kit/core';
import { type ReactNode, useId } from 'react';
import type { Stage } from '../../types';
import { StatusIcon } from './StatusIcon';
import './Column.css';

interface ColumnProps {
  stage: Stage;
  count: number;
  children: ReactNode;
}

export function Column({ stage, count, children }: ColumnProps) {
  const heading = useId();
  const { isOver, setNodeRef } = useDroppable({ id: stage.id, data: { stage } });

  return (
    <section className="board-column" data-stage={stage.id} aria-labelledby={heading}>
      <header className="board-column-header">
        <StatusIcon stage={stage.id} />
        <h2 id={heading}>{stage.label}</h2>
        <span className="board-column-count">{count}</span>
      </header>
      <div ref={setNodeRef} className="board-column-body" data-over={isOver || undefined}>
        <ul className="board-column-cards">{children}</ul>
        {count === 0 && <p className="board-column-empty">No tasks</p>}
      </div>
    </section>
  );
}
