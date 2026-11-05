import {
  type Announcements,
  DndContext,
  type DragCancelEvent,
  type DragEndEvent,
  type DragOverEvent,
  DragOverlay,
  type DragStartEvent,
  KeyboardSensor,
  type Modifier,
  PointerSensor,
  type UniqueIdentifier,
  useSensor,
  useSensors,
} from '@dnd-kit/core';
import { type ReactNode, useEffect, useId, useMemo, useRef, useState } from 'react';
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

const SILENT: Announcements = {
  onDragStart: () => undefined,
  onDragOver: () => undefined,
  onDragEnd: () => undefined,
  onDragCancel: () => undefined,
};

function title(id: UniqueIdentifier): string {
  return isPanelBox(id) ? boxTitle(id) : String(id);
}

/** The panel's boxes, which the viewer can drag by their grips, or move by keyboard, into the order they like. */
export function BoxList({ boxes, onMove, onDraggingChange }: BoxListProps) {
  const ids = boxes.map((box) => box.id);
  const current = useRef(ids);
  current.current = ids;
  const [active, setActive] = useState<PanelBoxId | null>(null);
  const [over, setOver] = useState<PanelBoxId | null>(null);
  const [announcement, setAnnouncement] = useState('');
  const announcedOver = useRef<UniqueIdentifier | null>(null);
  const dragging = useRef(onDraggingChange);
  dragging.current = onDraggingChange;
  const instructions = useId();
  const unrendered = useMemo(() => document.createElement('div'), []);
  const movable = ids.length > 1;

  useEffect(() => () => dragging.current?.(false), []);

  const steps = useMemo(() => ({ coordinateGetter: boxSteps(() => current.current) }), []);
  const sensors = useSensors(useSensor(PointerSensor, POINTER), useSensor(KeyboardSensor, steps));

  const place = (id: UniqueIdentifier) =>
    `position ${current.current.indexOf(id as PanelBoxId) + 1} of ${current.current.length}`;

  const finish = () => {
    setActive(null);
    setOver(null);
    dragging.current?.(false);
  };

  const start = ({ active }: DragStartEvent) => {
    setActive(isPanelBox(active.id) ? active.id : null);
    announcedOver.current = active.id;
    setAnnouncement(`Picked up ${title(active.id)}, at ${place(active.id)}.`);
    dragging.current?.(true);
  };

  const move = ({ active, over }: DragOverEvent) => {
    setOver(over && isPanelBox(over.id) ? over.id : null);
    const target = over?.id ?? null;
    if (target === announcedOver.current) return;
    announcedOver.current = target;
    setAnnouncement(
      over
        ? `${title(active.id)} would move to ${place(over.id)}.`
        : `${title(active.id)} is not over a place in the list.`
    );
  };

  const end = ({ active, over }: DragEndEvent) => {
    finish();
    const index = over && isPanelBox(over.id) ? ids.indexOf(over.id) : -1;
    if (!over || index === -1 || !isPanelBox(active.id)) {
      setAnnouncement(`${title(active.id)} put back at ${place(active.id)}.`);
      return;
    }
    setAnnouncement(`${title(active.id)} dropped at ${place(over.id)}.`);
    if (active.id !== over.id) onMove(active.id, index);
  };

  const cancel = ({ active }: DragCancelEvent) => {
    finish();
    setAnnouncement(`Moving ${title(active.id)} cancelled. It stays at ${place(active.id)}.`);
  };

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={nearestBox}
      accessibility={{ announcements: SILENT, container: unrendered }}
      onDragStart={start}
      onDragOver={move}
      onDragEnd={end}
      onDragCancel={cancel}
    >
      {boxes.map((box) => (
        <BoxSlot
          key={box.id}
          id={box.id}
          movable={movable}
          instructions={instructions}
          drop={dropSide(ids, active, over, box.id)}
        >
          {box.node}
        </BoxSlot>
      ))}
      <div className="chat-panel-announcer">
        <p id={instructions} hidden>
          {INSTRUCTIONS}
        </p>
        <div className="sr-only" aria-live="assertive" aria-atomic="true">
          {announcement}
        </div>
      </div>
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
