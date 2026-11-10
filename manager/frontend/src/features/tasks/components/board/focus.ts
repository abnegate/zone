const CARD_SELECTOR = 'li.task-card';

export const HANDLE_SELECTOR = '.task-card-handle';

export const TITLE_SELECTOR = '.task-card-open';

const COLUMN_SELECTOR = 'section.board-column';

export function focusLost(): boolean {
  const active = document.activeElement;
  return active === null || active === document.body;
}

export function cardIn(root: ParentNode | null, id: string): HTMLElement | undefined {
  if (!root) return undefined;
  return Array.from(root.querySelectorAll<HTMLElement>(CARD_SELECTOR)).find(
    (card) => card.dataset.taskId === id
  );
}

export function controlIn(
  root: ParentNode | null,
  id: string,
  selector: string
): HTMLElement | null {
  return cardIn(root, id)?.querySelector<HTMLElement>(selector) ?? null;
}

export function columnIn(root: ParentNode | null, stage: string): HTMLElement | undefined {
  if (!root) return undefined;
  return Array.from(root.querySelectorAll<HTMLElement>(COLUMN_SELECTOR)).find(
    (column) => column.dataset.stage === stage
  );
}
