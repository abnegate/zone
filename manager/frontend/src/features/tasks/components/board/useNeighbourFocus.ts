import { type RefObject, useLayoutEffect, useRef } from 'react';
import type { StageId } from '../../status';
import type { Task } from '../../types';
import { columnIn, controlIn, focusLost, TITLE_SELECTOR } from './focus';

type Groups = ReadonlyMap<StageId, readonly Task[]>;

function neighbour(cards: readonly Task[], index: number, present: ReadonlySet<string>) {
  return (
    cards.slice(index + 1).find((card) => present.has(card.id)) ??
    cards
      .slice(0, index)
      .reverse()
      .find((card) => present.has(card.id))
  );
}

export function useNeighbourFocus(root: RefObject<HTMLElement | null>, groups: Groups): void {
  const previous = useRef(groups);

  useLayoutEffect(() => {
    const before = previous.current;
    previous.current = groups;
    if (before === groups || !focusLost()) return;
    const present = new Set(
      Array.from(groups.values(), (cards) => cards.map((card) => card.id)).flat()
    );
    for (const [stage, cards] of before) {
      const index = cards.findIndex((card) => !present.has(card.id));
      if (index === -1) continue;
      const next = neighbour(cards, index, present);
      const target = next
        ? controlIn(root.current, next.id, TITLE_SELECTOR)
        : columnIn(root.current, stage);
      target?.focus();
      return;
    }
  }, [root, groups]);
}
