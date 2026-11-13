import type { Chat, ChatIndicator } from '../types';
import { attentionRank } from './indicators';
import { readAllowed, writeStored } from './storage';

export const CHAT_GROUP_BY = ['none', 'project', 'date'] as const;
export type ChatGroupBy = (typeof CHAT_GROUP_BY)[number];

/**
 * The orders the server lists chats in. Mirrors `db::chats::Sort`, which the
 * list query deserialises, so a name that is not here fails that query.
 */
export const SERVER_CHAT_SORT = [
  'updated_desc',
  'updated_asc',
  'created_desc',
  'created_asc',
  'title_asc',
  'title_desc',
] as const;
export type ServerChatSort = (typeof SERVER_CHAT_SORT)[number];

/**
 * What a reader can order the list by. `status` is the console's own: it is
 * read from each chat's indicator, which the list query knows nothing about.
 */
export const CHAT_SORT = [...SERVER_CHAT_SORT, 'status'] as const;
export type ChatSort = (typeof CHAT_SORT)[number];

export const CHAT_GROUP_OPTIONS: ReadonlyArray<{ value: ChatGroupBy; label: string }> = [
  { value: 'none', label: 'None' },
  { value: 'project', label: 'Project' },
  { value: 'date', label: 'Date' },
];

export const CHAT_SORT_OPTIONS: ReadonlyArray<{ value: ChatSort; label: string }> = [
  { value: 'status', label: 'Needs attention' },
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
  /**
   * What each chat is doing, by chat id. Only the status sort reads it, and a
   * chat with no reading yet sorts last rather than jumping to the top as the
   * readings arrive.
   */
  indicators?: Record<string, ChatIndicator>;
  now?: Date;
}

export function isChatGroupBy(value: string): value is ChatGroupBy {
  return (CHAT_GROUP_BY as readonly string[]).includes(value);
}

export function isChatSort(value: string): value is ChatSort {
  return (CHAT_SORT as readonly string[]).includes(value);
}

/**
 * The order to ask the server for.
 *
 * The list is paged server-side and the server knows nothing of a chat's
 * status, so "Needs attention" is ordered here out of readings the server
 * never sees. Asking it for that order would fail the whole list query, so it
 * sorts by last update — which is also what the attention order breaks its own
 * ties by, leaving the pages in a consistent order either way.
 */
export function listSort(sort: ChatSort): ServerChatSort {
  return sort === 'status' ? 'updated_desc' : sort;
}

export function readChatGroupBy(): ChatGroupBy {
  return readAllowed(CHAT_GROUP_STORAGE_KEY, CHAT_GROUP_BY, 'none');
}

export function readChatSort(): ChatSort {
  return readAllowed(CHAT_SORT_STORAGE_KEY, CHAT_SORT, 'updated_desc');
}

export function writeChatGroupBy(value: ChatGroupBy): void {
  writeStored(CHAT_GROUP_STORAGE_KEY, value);
}

export function writeChatSort(value: ChatSort): void {
  writeStored(CHAT_SORT_STORAGE_KEY, value);
}

export function arrangeChats(chats: readonly Chat[], options: ArrangeChatsOptions): ChatGroup[] {
  const { groupBy, sort, projectNames = {}, indicators = {}, now = new Date() } = options;
  const sorted = [...chats].sort((left, right) => compareChats(left, right, sort, indicators));

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

function compareChats(
  left: Chat,
  right: Chat,
  sort: ChatSort,
  indicators: Record<string, ChatIndicator>
): number {
  switch (sort) {
    case 'status':
      return (
        attentionRank(indicators[left.id]) - attentionRank(indicators[right.id]) ||
        timestamp(right.updated_at) - timestamp(left.updated_at) ||
        left.id.localeCompare(right.id)
      );
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
