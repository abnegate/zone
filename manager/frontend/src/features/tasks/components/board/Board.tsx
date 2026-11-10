import {
  type Announcements,
  DndContext,
  type DragCancelEvent,
  type DragEndEvent,
  type DragOverEvent,
  DragOverlay,
  type DragStartEvent,
  KeyboardSensor,
  type UniqueIdentifier,
  useSensor,
  useSensors,
} from '@dnd-kit/core';
import { useId, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useMediaQuery } from '../../../../shared/hooks';
import { groupByStage, STAGES, type StageId, stageOf } from '../../status';
import type { BoardActions, Renaming, Task } from '../../types';
import { Card } from './Card';
import { CardBody } from './CardBody';
import { CardSensor } from './CardSensor';
import { Column } from './Column';
import { destination, keyboardOptions, stageUnder } from './drop';
import { useCardFocus } from './useCardFocus';
import './Board.css';

export interface BoardProps {
  tasks: readonly Task[];
  projects: ReadonlyMap<string, string>;
  renaming: Renaming | null;
  removed: string | null;
  actions: BoardActions;
}

const POINTER = { activationConstraint: { distance: 5 } };

const REDUCED_MOTION = '(prefers-reduced-motion: reduce)';

const NO_PROJECTS = 'No projects';

const UNKNOWN_PROJECT = 'Unknown';

const INSTRUCTIONS =
  'To move a task, press space to pick it up, the left and right arrow keys to move it to another column, ' +
  'then space or enter to drop it, or escape to put it back.';

const SILENT: Announcements = {
  onDragStart: () => undefined,
  onDragOver: () => undefined,
  onDragEnd: () => undefined,
  onDragCancel: () => undefined,
};

function projectNames(task: Task, projects: ReadonlyMap<string, string>): string {
  if (task.project_ids.length === 0) return NO_PROJECTS;
  return task.project_ids.map((id) => projects.get(id) ?? UNKNOWN_PROJECT).join(', ');
}

function stageNamed(id: UniqueIdentifier | undefined) {
  return STAGES.find((stage) => stage.id === id);
}

function carried(data: { current?: { task?: Task } } | undefined): Task | undefined {
  return data?.current?.task;
}

export function Board({ tasks, projects, renaming, removed, actions }: BoardProps) {
  const groups = useMemo(() => groupByStage(tasks), [tasks]);
  const [active, setActive] = useState<UniqueIdentifier | null>(null);
  const [announcement, setAnnouncement] = useState('');
  const announcedOver = useRef<UniqueIdentifier | null>(null);
  const instructions = useId();
  const unrendered = useMemo(() => document.createElement('div'), []);
  const root = useRef<HTMLElement>(null);
  const release = useCardFocus(root, groups, removed);
  const reducedMotion = useMediaQuery(REDUCED_MOTION);
  const heading = useRef<StageId | null>(null);
  const keyboard = useMemo(() => keyboardOptions(reducedMotion, heading), [reducedMotion]);
  const sensors = useSensors(useSensor(CardSensor, POINTER), useSensor(KeyboardSensor, keyboard));

  const start = ({ active }: DragStartEvent) => {
    const task = carried(active.data);
    setActive(active.id);
    if (!task) return;
    const own = stageOf(task.status);
    announcedOver.current = own.id;
    heading.current = own.id;
    setAnnouncement(`Picked up ${task.title}, in ${own.label}.`);
  };

  const over = ({ active, over }: DragOverEvent) => {
    const task = carried(active.data);
    const target = over?.id ?? null;
    if (!task || target === announcedOver.current) return;
    announcedOver.current = target;
    const stage = stageNamed(target ?? undefined);
    setAnnouncement(
      stage ? `${task.title} is over ${stage.label}.` : `${task.title} is not over a column.`
    );
  };

  const end = ({ active, over, activatorEvent }: DragEndEvent) => {
    setActive(null);
    const task = carried(active.data);
    if (!task) return;
    const own = stageOf(task.status);
    const stage = stageNamed(over?.id);
    if (!stage) {
      setAnnouncement(`${task.title} put back in ${own.label}.`);
      return;
    }
    const status = destination(task, stage.id);
    if (status === null) {
      setAnnouncement(`${task.title} stays in ${own.label}.`);
      return;
    }
    setAnnouncement(`${task.title} moved to ${stage.label}.`);
    if (!(activatorEvent instanceof KeyboardEvent)) release();
    void actions.move(task, status).then((moved) => {
      if (!moved) setAnnouncement(`${task.title} could not be moved; back in ${own.label}.`);
    });
  };

  const cancel = ({ active }: DragCancelEvent) => {
    setActive(null);
    const task = carried(active.data);
    if (!task) return;
    setAnnouncement(`Moving ${task.title} cancelled. It stays in ${stageOf(task.status).label}.`);
  };

  const dragged = tasks.find((task) => task.id === active);

  return (
    <section
      ref={root}
      className="board"
      aria-label="Tasks"
      data-dragging={active !== null || undefined}
    >
      <DndContext
        id={instructions}
        sensors={sensors}
        collisionDetection={stageUnder}
        accessibility={{ announcements: SILENT, container: unrendered }}
        onDragStart={start}
        onDragOver={over}
        onDragEnd={end}
        onDragCancel={cancel}
      >
        {STAGES.map((stage) => {
          const cards = groups.get(stage.id) ?? [];
          return (
            <Column key={stage.id} stage={stage} count={cards.length}>
              {cards.map((task) => (
                <Card
                  key={task.id}
                  task={task}
                  projects={projectNames(task, projects)}
                  renaming={renaming?.id === task.id ? renaming.draft : null}
                  actions={actions}
                />
              ))}
            </Column>
          );
        })}
        <p id={instructions} hidden>
          {INSTRUCTIONS}
        </p>
        <div className="sr-only" aria-live="assertive" aria-atomic="true">
          {announcement}
        </div>
        {createPortal(
          <DragOverlay dropAnimation={null} style={{ zIndex: 'var(--ui-layer-popover)' }}>
            {dragged && (
              <div className="task-card task-card-overlay">
                <CardBody
                  task={dragged}
                  projects={projectNames(dragged, projects)}
                  title={<span>{dragged.title}</span>}
                />
              </div>
            )}
          </DragOverlay>,
          document.body
        )}
      </DndContext>
    </section>
  );
}
