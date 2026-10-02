import type { Chat } from '../types';

export const CHAT_GROUP_BY = ['none', 'project', 'date'] as const;
export type ChatGroupBy = (typeof CHAT_GROUP_BY)[number];

export const CHAT_SORT = [
  'updated_desc',
  'updated_asc',
  'created_desc',
  'created_asc',
  'title_asc',
  'title_desc',
] as const;
export type ChatSort = (typeof CHAT_SORT)[number];

export const CHAT_GROUP_OPTIONS: ReadonlyArray<{ value: ChatGroupBy; label: string }> = [
  { value: 'none', label: 'None' },
  { value: 'project', label: 'Project' },
  { value: 'date', label: 'Date' },
];

export const CHAT_SORT_OPTIONS: ReadonlyArray<{ value: ChatSort; label: string }> = [
  { value: 'updated_desc', label: 'Last updated' },
  { value: 'updated_asc', label: 'Oldest updated' },
  { value: 'created_desc', label: 'Newest' },
  { value: 'created_asc', label: 'Oldest' },
  { value: 'title_asc', label: 'Title A-Z' },
  { value: 'title_desc', label: 'Title Z-A' },
];

export const CHAT_GROUP_STORAGE_KEY = 'manager_chats_group';
export const CHAT_SORT_STORAGE_KEY = 'manager_chats_sort';

export const NO_PROJECT_LABEL = 'No project';
export const UNKNOWN_PROJECT_LABEL = 'Unknown project';

const PROJECT_NONE_KEY = 'project:none';
const PROJECT_UNKNOWN_KEY = 'project:unknown';

export interface ChatGroup {
  key: string;
  label: string | null;
  chats: Chat[];
}

export interface ArrangeChatsOptions {
  groupBy: ChatGroupBy;
  sort: ChatSort;
  projectNames?: Record<string, string>;
  now?: Date;
}

export function isChatGroupBy(value: string): value is ChatGroupBy {
  return (CHAT_GROUP_BY as readonly string[]).includes(value);
}

export function isChatSort(value: string): value is ChatSort {
  return (CHAT_SORT as readonly string[]).includes(value);
}

export function readChatGroupBy(): ChatGroupBy {
  return readAllowed(CHAT_GROUP_STORAGE_KEY, CHAT_GROUP_BY, 'none');
}

export function readChatSort(): ChatSort {
  return readAllowed(CHAT_SORT_STORAGE_KEY, CHAT_SORT, 'updated_desc');
}

export function writeChatGroupBy(value: ChatGroupBy): void {
  localStorage.setItem(CHAT_GROUP_STORAGE_KEY, value);
}

export function writeChatSort(value: ChatSort): void {
  localStorage.setItem(CHAT_SORT_STORAGE_KEY, value);
}

export function arrangeChats(chats: readonly Chat[], options: ArrangeChatsOptions): ChatGroup[] {
  const { groupBy, sort, projectNames = {}, now = new Date() } = options;
  const sorted = [...chats].sort((left, right) => compareChats(left, right, sort));

  if (groupBy === 'none') {
    return [{ key: 'all', label: null, chats: sorted }];
  }

  const buckets = new Map<string, ChatGroup>();
  for (const chat of sorted) {
    const { key, label } = groupOf(chat, groupBy, projectNames, now, sort);
    const bucket = buckets.get(key);
    if (bucket) {
      bucket.chats.push(chat);
    } else {
      buckets.set(key, { key, label, chats: [chat] });
    }
  }

  return [...buckets.values()].sort((left, right) => compareGroups(left, right, groupBy, sort));
}

function readAllowed<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  const value = localStorage.getItem(key);
  return value !== null && (allowed as readonly string[]).includes(value) ? (value as T) : fallback;
}

function compareChats(left: Chat, right: Chat, sort: ChatSort): number {
  switch (sort) {
    case 'updated_desc':
      return (
        timestamp(right.updated_at) - timestamp(left.updated_at) || left.id.localeCompare(right.id)
      );
    case 'updated_asc':
      return (
        timestamp(left.updated_at) - timestamp(right.updated_at) || left.id.localeCompare(right.id)
      );
    case 'created_desc':
      return (
        timestamp(right.created_at) - timestamp(left.created_at) || left.id.localeCompare(right.id)
      );
    case 'created_asc':
      return (
        timestamp(left.created_at) - timestamp(right.created_at) || left.id.localeCompare(right.id)
      );
    case 'title_asc':
      return (
        left.title.localeCompare(right.title, undefined, { sensitivity: 'base' }) ||
        left.id.localeCompare(right.id)
      );
    case 'title_desc':
      return (
        right.title.localeCompare(left.title, undefined, { sensitivity: 'base' }) ||
        left.id.localeCompare(right.id)
      );
  }
}

function compareGroups(
  left: ChatGroup,
  right: ChatGroup,
  groupBy: ChatGroupBy,
  sort: ChatSort
): number {
  if (groupBy === 'project') {
    const rank = (key: string) => {
      if (key === PROJECT_NONE_KEY) return 2;
      if (key === PROJECT_UNKNOWN_KEY) return 1;
      return 0;
    };
    const difference = rank(left.key) - rank(right.key);
    if (difference !== 0) return difference;
    return (left.label ?? '').localeCompare(right.label ?? '', undefined, { sensitivity: 'base' });
  }

  const direction = sort === 'updated_asc' || sort === 'created_asc' ? 1 : -1;
  return left.key.localeCompare(right.key) * direction;
}

function groupOf(
  chat: Chat,
  groupBy: ChatGroupBy,
  projectNames: Record<string, string>,
  now: Date,
  sort: ChatSort
): { key: string; label: string } {
  if (groupBy === 'project') {
    if (!chat.project_id) {
      return { key: PROJECT_NONE_KEY, label: NO_PROJECT_LABEL };
    }
    const name = projectNames[chat.project_id];
    if (name) {
      return { key: `project:${chat.project_id}`, label: name };
    }
    return { key: PROJECT_UNKNOWN_KEY, label: UNKNOWN_PROJECT_LABEL };
  }

  const field =
    sort === 'created_desc' || sort === 'created_asc' ? chat.created_at : chat.updated_at;
  const date = parseDate(field);
  return { key: `date:${localDayKey(date)}`, label: dateGroupLabel(date, now) };
}

function timestamp(value: string): number {
  const time = Date.parse(value);
  return Number.isNaN(time) ? 0 : time;
}

function parseDate(value: string): Date {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? new Date(0) : date;
}

function localDayKey(date: Date): string {
  const year = String(date.getFullYear());
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

function startOfLocalDay(date: Date): number {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

function calendarDaysBetween(later: Date, earlier: Date): number {
  return Math.round((startOfLocalDay(later) - startOfLocalDay(earlier)) / 86_400_000);
}

function dateGroupLabel(date: Date, now: Date): string {
  const days = calendarDaysBetween(now, date);
  if (days === 0) return 'Today';
  if (days === 1) return 'Yesterday';
  if (days > 1 && days < 7) {
    return date.toLocaleDateString([], { weekday: 'short' });
  }
  if (date.getFullYear() === now.getFullYear()) {
    return date.toLocaleDateString([], { month: 'short', day: 'numeric' });
  }
  return date.toLocaleDateString([], { month: 'short', day: 'numeric', year: 'numeric' });
}
