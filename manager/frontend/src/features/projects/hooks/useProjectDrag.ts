import type { DraggableAttributes, DraggableSyntheticListeners } from '@dnd-kit/core';
import { createContext, useContext } from 'react';

export interface ProjectDrag {
  setHandle: (element: HTMLElement | null) => void;
  attributes: DraggableAttributes;
  listeners: DraggableSyntheticListeners;
  instructions: string;
}

export const ProjectDragContext = createContext<ProjectDrag | null>(null);

/** How the project card being drawn can be moved, when the list lets it move. */
export function useProjectDrag(): ProjectDrag | null {
  return useContext(ProjectDragContext);
}
