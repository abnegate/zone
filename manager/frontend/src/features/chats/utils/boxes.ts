/** Every box the chat panel can hold, in the order a viewer sees them until they move one. */
export const PANEL_BOXES = [
  { id: 'changes', title: 'Changes' },
  { id: 'subagents', title: 'Agents' },
  { id: 'tasks', title: 'Tasks' },
  { id: 'sources', title: 'Sources' },
  { id: 'gallery', title: 'Gallery' },
] as const;

export type PanelBoxId = (typeof PANEL_BOXES)[number]['id'];

export type DropSide = 'before' | 'after';

export const DEFAULT_ORDER: readonly PanelBoxId[] = PANEL_BOXES.map((box) => box.id);

export function isPanelBox(id: unknown): id is PanelBoxId {
  return DEFAULT_ORDER.includes(id as PanelBoxId);
}

export function boxTitle(id: PanelBoxId): string {
  return PANEL_BOXES.find((box) => box.id === id)?.title ?? id;
}

/** A saved order as the ids it lists, or none when it cannot be read. */
export function parseOrder(stored: string | null): string[] {
  if (!stored) return [];
  try {
    const parsed: unknown = JSON.parse(stored);
    return Array.isArray(parsed) ? parsed.filter((id) => typeof id === 'string') : [];
  } catch {
    return [];
  }
}

/**
 * The boxes in a viewer's saved order. A box they never placed, such as one added
 * since, follows the ones they did in its default order; one that no longer exists is dropped.
 */
export function mergeOrder(
  saved: readonly string[],
  boxes: readonly PanelBoxId[] = DEFAULT_ORDER
): PanelBoxId[] {
  const placed = [...new Set(saved)].filter((id): id is PanelBoxId =>
    boxes.includes(id as PanelBoxId)
  );
  return [...placed, ...boxes.filter((id) => !placed.includes(id))];
}

/**
 * Moves a box to a new place among the boxes being shown. The boxes left out, which
 * have nothing to show, keep their places, so they come back where the viewer put them.
 */
export function moveBox(
  order: readonly PanelBoxId[],
  shown: readonly PanelBoxId[],
  id: PanelBoxId,
  index: number
): PanelBoxId[] {
  const from = shown.indexOf(id);
  if (from === -1) return [...order];
  const to = Math.max(0, Math.min(shown.length - 1, index));
  const moved = shown.filter((box) => box !== id);
  moved.splice(to, 0, id);
  let next = 0;
  return order.map((box) => (shown.includes(box) ? moved[next++] : box));
}

export function dropSide(
  shown: readonly PanelBoxId[],
  active: PanelBoxId | null,
  over: PanelBoxId | null,
  id: PanelBoxId
): DropSide | null {
  if (!active || !over || over !== id || over === active) return null;
  return shown.indexOf(over) < shown.indexOf(active) ? 'before' : 'after';
}
