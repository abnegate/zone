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
import { type ReactNode, useId, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { nearestSlot, slotSteps } from '../../../shared/utils/listDrop';
import type { Project } from '../types';
import { type DropSide, ProjectSlot } from './ProjectSlot';

export interface ProjectListProps {
  projects: readonly Project[];
  movable: boolean;
  renderCard: (project: Project) => ReactNode;
  onMove: (id: string, index: number) => void;
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
  'To move a project, press space or enter to pick it up, the up and down arrow keys to move it, ' +
  'then space or enter to drop it, or escape to put it back.';

const SILENT: Announcements = {
  onDragStart: () => undefined,
  onDragOver: () => undefined,
  onDragEnd: () => undefined,
  onDragCancel: () => undefined,
};

function dropSide(
  ids: readonly string[],
  active: string | null,
  over: string | null,
  id: string
): DropSide | null {
  if (!active || !over || over !== id || over === active) return null;
  return ids.indexOf(over) < ids.indexOf(active) ? 'before' : 'after';
}

/** The project cards, which can be dragged by their grips, or moved by keyboard, into a custom order. */
export function ProjectList({ projects, movable, renderCard, onMove }: ProjectListProps) {
  const ids = projects.map((project) => project.id);
  const current = useRef(ids);
  current.current = ids;
  const names = useRef(new Map<UniqueIdentifier, string>());
  names.current = new Map(projects.map((project) => [project.id, project.name]));
  const [active, setActive] = useState<string | null>(null);
  const [over, setOver] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState('');
  const announcedOver = useRef<UniqueIdentifier | null>(null);
  const instructions = useId();
  const unrendered = useMemo(() => document.createElement('div'), []);
  const moving = movable && ids.length > 1;

  const steps = useMemo(() => ({ coordinateGetter: slotSteps(() => current.current) }), []);
  const sensors = useSensors(useSensor(PointerSensor, POINTER), useSensor(KeyboardSensor, steps));

  const name = (id: UniqueIdentifier) => names.current.get(id) ?? String(id);
  const place = (id: UniqueIdentifier) =>
    `position ${current.current.indexOf(String(id)) + 1} of ${current.current.length}`;
  const listed = (id: UniqueIdentifier | undefined): id is string =>
    typeof id === 'string' && current.current.includes(id);

  const finish = () => {
    setActive(null);
    setOver(null);
  };

  const start = ({ active }: DragStartEvent) => {
    setActive(listed(active.id) ? active.id : null);
    announcedOver.current = active.id;
    setAnnouncement(`Picked up ${name(active.id)}, at ${place(active.id)}.`);
  };

  const move = ({ active, over }: DragOverEvent) => {
    setOver(listed(over?.id) ? over.id : null);
    const target = over?.id ?? null;
    if (target === announcedOver.current) return;
    announcedOver.current = target;
    setAnnouncement(
      over
        ? `${name(active.id)} would move to ${place(over.id)}.`
        : `${name(active.id)} is not over a place in the list.`
    );
  };

  const end = ({ active, over }: DragEndEvent) => {
    finish();
    if (!over || !listed(over.id) || !listed(active.id)) {
      setAnnouncement(`${name(active.id)} put back at ${place(active.id)}.`);
      return;
    }
    setAnnouncement(`${name(active.id)} dropped at ${place(over.id)}.`);
    if (active.id !== over.id) onMove(active.id, ids.indexOf(over.id));
  };

  const cancel = ({ active }: DragCancelEvent) => {
    finish();
    setAnnouncement(`Moving ${name(active.id)} cancelled. It stays at ${place(active.id)}.`);
  };

  const dragged = projects.find((project) => project.id === active);

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={nearestSlot}
      accessibility={{ announcements: SILENT, container: unrendered }}
      onDragStart={start}
      onDragOver={move}
      onDragEnd={end}
      onDragCancel={cancel}
    >
      {projects.map((project) => (
        <ProjectSlot
          key={project.id}
          id={project.id}
          movable={moving}
          instructions={instructions}
          drop={dropSide(ids, active, over, project.id)}
        >
          {renderCard(project)}
        </ProjectSlot>
      ))}
      <p id={instructions} hidden>
        {INSTRUCTIONS}
      </p>
      <div className="sr-only" aria-live="assertive" aria-atomic="true">
        {announcement}
      </div>
      {createPortal(
        <DragOverlay
          className="project-drag-overlay"
          dropAnimation={null}
          modifiers={GHOST_MODIFIERS}
          style={{ zIndex: 'var(--ui-layer-popover)' }}
        >
          {dragged && <div className="project-card-ghost">{dragged.name}</div>}
        </DragOverlay>,
        document.body
      )}
    </DndContext>
  );
}
