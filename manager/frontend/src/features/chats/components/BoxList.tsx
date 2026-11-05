import {
  type Announcements,
  DndContext,
  type DragEndEvent,
  DragOverlay,
  type DragStartEvent,
  KeyboardSensor,
  type Modifier,
  PointerSensor,
  type UniqueIdentifier,
  useSensor,
  useSensors,
} from '@dnd-kit/core';
import { type ReactNode, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { boxSteps, nearestBox } from '../utils/boxDrop';
import { boxTitle, dropSide, isPanelBox, type PanelBoxId } from '../utils/boxes';
import { BoxSlot } from './BoxSlot';

export interface ListedBox {
  id: PanelBoxId;
  node: ReactNode;
}

export interface BoxListProps {
  boxes: readonly ListedBox[];
  onMove: (id: PanelBoxId, index: number) => void;
  onDraggingChange?: (dragging: boolean) => void;
}

const POINTER = { activationConstraint: { distance: 4 } };

const GHOST_OFFSET = { x: 16, y: 24 };

const besidePointer: Modifier = ({ transform }) => ({
  ...transform,
  x: transform.x + GHOST_OFFSET.x,
  y: transform.y + GHOST_OFFSET.y,
});

const GHOST_MODIFIERS = [besidePointer];

const INSTRUCTIONS =
  'To move a box, press space or enter to pick it up, the up and down arrow keys to move it, ' +
  'then space or enter to drop it, or escape to put it back.';

/** The panel's boxes, which the viewer can drag by their grips, or move by keyboard, into the order they like. */
export function BoxList({ boxes, onMove, onDraggingChange }: BoxListProps) {
  const ids = boxes.map((box) => box.id);
  const current = useRef(ids);
  current.current = ids;
  const [active, setActive] = useState<PanelBoxId | null>(null);
  const [over, setOver] = useState<PanelBoxId | null>(null);
  const [announcer, setAnnouncer] = useState<HTMLDivElement | null>(null);
  const movable = ids.length > 1;

  const steps = useMemo(() => ({ coordinateGetter: boxSteps(() => current.current) }), []);
  const sensors = useSensors(useSensor(PointerSensor, POINTER), useSensor(KeyboardSensor, steps));

  const announcements = useMemo<Announcements>(() => {
    let announcedOver: UniqueIdentifier | null = null;
    const place = (id: UniqueIdentifier) =>
      `position ${current.current.indexOf(id as PanelBoxId) + 1} of ${current.current.length}`;
    const title = (id: UniqueIdentifier) => (isPanelBox(id) ? boxTitle(id) : String(id));
    return {
      onDragStart: ({ active }) => {
        announcedOver = active.id;
        return `Picked up ${title(active.id)}, at ${place(active.id)}.`;
      },
      onDragOver: ({ active, over }) => {
        if ((over?.id ?? null) === announcedOver) return undefined;
        announcedOver = over?.id ?? null;
        return over
          ? `${title(active.id)} would move to ${place(over.id)}.`
          : `${title(active.id)} is not over a place in the list.`;
      },
      onDragEnd: ({ active, over }) =>
        over
          ? `${title(active.id)} dropped at ${place(over.id)}.`
          : `${title(active.id)} put back at ${place(active.id)}.`,
      onDragCancel: ({ active }) =>
        `Moving ${title(active.id)} cancelled. It stays at ${place(active.id)}.`,
    };
  }, []);

  const start = ({ active }: DragStartEvent) => {
    setActive(isPanelBox(active.id) ? active.id : null);
    onDraggingChange?.(true);
  };

  const finish = () => {
    setActive(null);
    setOver(null);
    onDraggingChange?.(false);
  };

  const end = ({ active, over }: DragEndEvent) => {
    finish();
    if (!over || !isPanelBox(active.id) || !isPanelBox(over.id)) return;
    if (active.id !== over.id) onMove(active.id, ids.indexOf(over.id));
  };

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={nearestBox}
      accessibility={{
        announcements,
        container: announcer ?? undefined,
        screenReaderInstructions: { draggable: INSTRUCTIONS },
      }}
      onDragStart={start}
      onDragOver={({ over }) => setOver(over && isPanelBox(over.id) ? over.id : null)}
      onDragEnd={end}
      onDragCancel={finish}
    >
      {boxes.map((box) => (
        <BoxSlot
          key={box.id}
          id={box.id}
          movable={movable}
          drop={dropSide(ids, active, over, box.id)}
        >
          {box.node}
        </BoxSlot>
      ))}
      <div ref={setAnnouncer} className="chat-panel-announcer" />
      {createPortal(
        <DragOverlay
          className="chat-panel-drag-overlay"
          dropAnimation={null}
          modifiers={GHOST_MODIFIERS}
          style={{ zIndex: 'var(--ui-layer-popover)' }}
        >
          {active && (
            <div className="chat-panel-box-ghost">
              <span className="chat-panel-box-title">{boxTitle(active)}</span>
            </div>
          )}
        </DragOverlay>,
        document.body
      )}
    </DndContext>
  );
}
