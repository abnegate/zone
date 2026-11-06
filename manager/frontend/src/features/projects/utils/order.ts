import type { Project, ProjectStatus } from '../types';

export const PROJECT_SORTS = ['custom', 'recent', 'added', 'name', 'status'] as const;
export type ProjectSort = (typeof PROJECT_SORTS)[number];

export const DEFAULT_PROJECT_SORT: ProjectSort = 'custom';

export const PROJECT_SORT_OPTIONS: ReadonlyArray<{ value: ProjectSort; label: string }> = [
  { value: 'custom', label: 'Custom' },
  { value: 'recent', label: 'Most recently used' },
  { value: 'added', label: 'Date added' },
  { value: 'name', label: 'Name' },
  { value: 'status', label: 'Status' },
];

const STATUS_RANK: Record<ProjectStatus, number> = {
  active: 0,
  on_hold: 1,
  cancelled: 2,
};

/** Where one viewer's project sort is kept, so people sharing a browser keep their own. */
export function sortKey(userId: string | null | undefined): string {
  return `zone:projects:${userId || 'signed-out'}:sort`;
}

export function isProjectSort(value: unknown): value is ProjectSort {
  return (PROJECT_SORTS as readonly unknown[]).includes(value);
}

export function readSort(key: string): ProjectSort {
  try {
    const stored = localStorage.getItem(key);
    return isProjectSort(stored) ? stored : DEFAULT_PROJECT_SORT;
  } catch {
    return DEFAULT_PROJECT_SORT;
  }
}

export function writeSort(key: string, sort: ProjectSort): void {
  try {
    localStorage.setItem(key, sort);
  } catch {
    return;
  }
}

function time(value: string | null | undefined): number {
  const parsed = value ? Date.parse(value) : Number.NaN;
  return Number.isNaN(parsed) ? Number.NEGATIVE_INFINITY : parsed;
}

function byName(left: Project, right: Project): number {
  return left.name.localeCompare(right.name, undefined, { sensitivity: 'base' });
}

function newest(left: Project, right: Project): number {
  return time(right.created_at) - time(left.created_at) || left.id.localeCompare(right.id);
}

function custom(left: Project, right: Project): number {
  return left.position - right.position;
}

const COMPARE: Record<ProjectSort, (left: Project, right: Project) => number> = {
  custom,
  recent: (left, right) =>
    time(right.last_used_at) - time(left.last_used_at) || newest(left, right),
  added: newest,
  name: (left, right) => byName(left, right) || newest(left, right),
  status: (left, right) =>
    STATUS_RANK[left.status] - STATUS_RANK[right.status] || custom(left, right),
};

/**
 * The projects as the chosen sort lays them out; projects never used follow the ones that
 * were. Ties keep the order the server listed them in, which is the custom one.
 */
export function sortProjects(projects: readonly Project[], sort: ProjectSort): Project[] {
  return [...projects].sort(COMPARE[sort]);
}

/** The ids in a list once the one at `id` is moved to `index`. */
export function moveId(ids: readonly string[], id: string, index: number): string[] {
  if (!ids.includes(id)) return [...ids];
  const moved = ids.filter((other) => other !== id);
  moved.splice(Math.max(0, Math.min(moved.length, index)), 0, id);
  return moved;
}

/**
 * The workspace's projects once the ones listed in `order` take, in that order, the
 * places they held between them. Mirrors the server, so the list shows a drop at once.
 */
export function applyOrder(projects: readonly Project[], order: readonly string[]): Project[] {
  const current = sortProjects(projects, 'custom');
  const byId = new Map(current.map((project) => [project.id, project]));
  const placed = [...new Set(order)].flatMap((id) => byId.get(id) ?? []);
  const slots = current.filter((project) => placed.includes(project));
  const moved = new Map(
    placed.map((project, index) => [project.id, { ...project, position: slots[index].position }])
  );
  return sortProjects(
    current.map((project) => moved.get(project.id) ?? project),
    'custom'
  );
}
