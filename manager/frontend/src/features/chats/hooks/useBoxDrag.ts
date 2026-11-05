import type { DraggableAttributes, DraggableSyntheticListeners } from '@dnd-kit/core';
import { createContext, useContext } from 'react';

export interface BoxDrag {
  setHandle: (element: HTMLElement | null) => void;
  attributes: DraggableAttributes;
  listeners: DraggableSyntheticListeners;
  dragging: boolean;
  instructions: string;
}

export const BoxDragContext = createContext<BoxDrag | null>(null);

/** How the panel box being drawn can be moved, when the panel lets it move. */
export function useBoxDrag(): BoxDrag | null {
  return useContext(BoxDragContext);
}
