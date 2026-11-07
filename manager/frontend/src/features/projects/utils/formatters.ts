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

/** A card shows the date its sort orders by, so the order and the dates agree. */
export function cardDate(project: Project, sort: ProjectSort): string {
  switch (sort) {
    case 'recent':
      return project.last_used_at ? formatDate(project.last_used_at) : NEVER_USED;
    case 'added':
      return formatDate(project.created_at);
    default:
      return formatDate(project.updated_at);
  }
}
