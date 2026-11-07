import type { Project } from '../types';
import type { ProjectSort } from './order';

const NEVER_USED = 'Never used';

/**
 * Format a date string for display
 * @param dateStr ISO date string
 * @returns Formatted date string (e.g., "Jan 15, 2024")
 */
export function formatDate(dateStr: string): string {
  const date = new Date(dateStr);
  return date.toLocaleDateString([], { month: 'short', day: 'numeric', year: 'numeric' });
}

const updated = (project: Project): string => formatDate(project.updated_at);

const CARD_DATE: Record<ProjectSort, (project: Project) => string> = {
  custom: updated,
  recent: (project) => (project.last_used_at ? formatDate(project.last_used_at) : NEVER_USED),
  added: (project) => formatDate(project.created_at),
  name: updated,
  status: updated,
};

/** A card shows the date its sort orders by, so the order and the dates agree. */
export function cardDate(project: Project, sort: ProjectSort): string {
  return CARD_DATE[sort](project);
}
