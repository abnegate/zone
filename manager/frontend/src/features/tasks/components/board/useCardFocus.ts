import { type RefObject, useCallback, useEffect, useLayoutEffect, useRef } from 'react';
import type { StageId } from '../../status';
import type { Task } from '../../types';

type Groups = ReadonlyMap<StageId, readonly Task[]>;

interface Held {
  id: string;
  control: string;
}

const CARD = 'li[data-task-id]';

const COLUMN = 'section[data-stage]';

const TITLE = '.task-card-open';

const MENU = '.task-card-menu button';

const CONTROLS = ['.task-card-handle', TITLE, '.task-card-pr', MENU];

const POPUP = '[role="menu"]';

const POPUP_OWNER = 'aria-labelledby';

function lost(): boolean {
  const active = document.activeElement;
  return active === null || active === document.body;
}

function cardIn(board: HTMLElement | null, id: string): HTMLElement | undefined {
  return Array.from(board?.querySelectorAll<HTMLElement>(CARD) ?? []).find(
    (card) => card.dataset.taskId === id
  );
}

function controlIn(board: HTMLElement | null, { id, control }: Held): HTMLElement | null {
  return cardIn(board, id)?.querySelector<HTMLElement>(control) ?? null;
}

function columnIn(board: HTMLElement | null, stage: StageId): HTMLElement | undefined {
  return Array.from(board?.querySelectorAll<HTMLElement>(COLUMN) ?? []).find(
    (column) => column.dataset.stage === stage
  );
}

function owner(target: HTMLElement): HTMLElement {
  const trigger = target.closest(POPUP)?.getAttribute(POPUP_OWNER);
  return (trigger && document.getElementById(trigger)) || target;
}

function neighbour(cards: readonly Task[], index: number, present: ReadonlySet<string>) {
  return (
    cards.slice(index + 1).find((card) => present.has(card.id)) ??
    cards
      .slice(0, index)
      .reverse()
      .find((card) => present.has(card.id))
  );
}

function ids(groups: Groups): Set<string> {
  return new Set(Array.from(groups.values(), (cards) => cards.map((card) => card.id)).flat());
}

export function useCardFocus(
  board: RefObject<HTMLElement | null>,
  groups: Groups,
  removed: string | null
): () => void {
  const held = useRef<Held | null>(null);
  const previous = useRef(groups);
  const departed = useRef(groups);
  const settled = useRef<string | null>(null);

  useLayoutEffect(() => {
    const before = previous.current;
    previous.current = groups;
    const present = ids(groups);
    if (before !== groups && Array.from(ids(before)).some((id) => !present.has(id))) {
      departed.current = before;
    }
    const kept = lost() && held.current && controlIn(board.current, held.current);
    if (kept) {
      kept.focus();
      return;
    }
    if (removed === null || removed === settled.current || present.has(removed)) return;
    settled.current = removed;
    if (!lost()) return;
    for (const [stage, cards] of departed.current) {
      const index = cards.findIndex((card) => card.id === removed);
      if (index === -1) continue;
      const next = neighbour(cards, index, present);
      const target = next
        ? controlIn(board.current, { id: next.id, control: TITLE })
        : columnIn(board.current, stage);
      target?.focus();
      return;
    }
  });

  useEffect(() => {
    const hold = (event: FocusEvent) => {
      const source = event.target instanceof HTMLElement ? owner(event.target) : null;
      const id = source?.closest<HTMLElement>(CARD)?.dataset.taskId;
      const control =
        source && id && board.current?.contains(source)
          ? CONTROLS.find((selector) => source.matches(selector))
          : undefined;
      held.current = id && control ? { id, control } : null;
    };
    const release = () => {
      held.current = null;
    };
    document.addEventListener('focusin', hold);
    document.addEventListener('pointerdown', release, true);
    return () => {
      document.removeEventListener('focusin', hold);
      document.removeEventListener('pointerdown', release, true);
    };
  }, [board]);

  return useCallback(() => {
    held.current = null;
  }, []);
}
