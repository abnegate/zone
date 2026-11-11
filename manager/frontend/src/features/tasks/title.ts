import type { Task } from './types';

export const TITLE_POLL_MS = 2000;

const TITLE_WAIT_MS = 120_000;

export function titlePoll(tasks: readonly Task[] | undefined): number | false {
  return tasks?.some((task) => awaitsTitle(task)) ? TITLE_POLL_MS : false;
}

export function awaitsTitle(task: Task, now: number = Date.now()): boolean {
  if (!task.title_pending || !task.created_at) return false;
  return now - Date.parse(task.created_at) < TITLE_WAIT_MS;
}
