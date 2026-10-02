import { beforeEach, describe, expect, it } from 'bun:test';
import type { Chat } from '../types';
import {
  arrangeChats,
  CHAT_GROUP_STORAGE_KEY,
  CHAT_SORT_STORAGE_KEY,
  NO_PROJECT_LABEL,
  readChatGroupBy,
  readChatSort,
  UNKNOWN_PROJECT_LABEL,
  writeChatGroupBy,
  writeChatSort,
} from './arrange';

const now = new Date(2026, 9, 2, 15, 0, 0);

const iso = (year: number, month: number, day: number, hour = 12): string =>
  new Date(year, month, day, hour, 0, 0).toISOString();

const chat = (overrides: Partial<Chat> & Pick<Chat, 'id' | 'title'>): Chat => ({
  model_name: 'llama2',
  archived: false,
  agent_enabled: false,
  created_at: iso(2026, 0, 1),
  updated_at: iso(2026, 9, 2),
  ...overrides,
});

describe('arrangeChats', () => {
  it('returns one unlabeled group sorted by last update', () => {
    const chats = [
      chat({ id: 'old', title: 'Old', updated_at: iso(2026, 8, 1) }),
      chat({ id: 'new', title: 'New', updated_at: iso(2026, 9, 2) }),
      chat({ id: 'mid', title: 'Mid', updated_at: iso(2026, 9, 1) }),
    ];

    expect(arrangeChats(chats, { groupBy: 'none', sort: 'updated_desc' })).toEqual([
      {
        key: 'all',
        label: null,
        chats: [chats[1], chats[2], chats[0]],
      },
    ]);
  });

  it('sorts by created date, title, and the reverse of each', () => {
    const chats = [
      chat({
        id: 'b',
        title: 'Beta',
        created_at: iso(2026, 0, 2),
        updated_at: iso(2026, 9, 1),
      }),
      chat({
        id: 'a',
        title: 'Alpha',
        created_at: iso(2026, 0, 3),
        updated_at: iso(2026, 8, 1),
      }),
      chat({
        id: 'c',
        title: 'Gamma',
        created_at: iso(2026, 0, 1),
        updated_at: iso(2026, 9, 2),
      }),
    ];

    expect(ids(arrangeChats(chats, { groupBy: 'none', sort: 'created_desc' }))).toEqual([
      'a',
      'b',
      'c',
    ]);
    expect(ids(arrangeChats(chats, { groupBy: 'none', sort: 'created_asc' }))).toEqual([
      'c',
      'b',
      'a',
    ]);
    expect(ids(arrangeChats(chats, { groupBy: 'none', sort: 'title_asc' }))).toEqual([
      'a',
      'b',
      'c',
    ]);
    expect(ids(arrangeChats(chats, { groupBy: 'none', sort: 'title_desc' }))).toEqual([
      'c',
      'b',
      'a',
    ]);
    expect(ids(arrangeChats(chats, { groupBy: 'none', sort: 'updated_asc' }))).toEqual([
      'a',
      'b',
      'c',
    ]);
  });

  it('groups by project name, unknown ids, and chats with no project last', () => {
    const chats = [
      chat({ id: 'none', title: 'Loose', project_id: null }),
      chat({ id: 'zeta', title: 'Zeta chat', project_id: 'proj-zeta' }),
      chat({ id: 'gone', title: 'Gone', project_id: 'missing' }),
      chat({
        id: 'alpha-old',
        title: 'Older alpha',
        project_id: 'proj-alpha',
        updated_at: iso(2026, 8, 1),
      }),
      chat({
        id: 'alpha-new',
        title: 'Newer alpha',
        project_id: 'proj-alpha',
        updated_at: iso(2026, 9, 2),
      }),
    ];

    const groups = arrangeChats(chats, {
      groupBy: 'project',
      sort: 'updated_desc',
      projectNames: { 'proj-alpha': 'Alpha', 'proj-zeta': 'Zeta' },
    });

    expect(groups.map((group) => group.label)).toEqual([
      'Alpha',
      'Zeta',
      UNKNOWN_PROJECT_LABEL,
      NO_PROJECT_LABEL,
    ]);
    expect(groups[0].chats.map((item) => item.id)).toEqual(['alpha-new', 'alpha-old']);
    expect(groups[1].chats.map((item) => item.id)).toEqual(['zeta']);
    expect(groups[2].chats.map((item) => item.id)).toEqual(['gone']);
    expect(groups[3].chats.map((item) => item.id)).toEqual(['none']);
  });

  it('buckets unknown project ids together', () => {
    const chats = [
      chat({ id: 'one', title: 'One', project_id: 'gone-1' }),
      chat({ id: 'two', title: 'Two', project_id: 'gone-2' }),
    ];

    const groups = arrangeChats(chats, { groupBy: 'project', sort: 'title_asc' });
    expect(groups).toHaveLength(1);
    expect(groups[0].label).toBe(UNKNOWN_PROJECT_LABEL);
    expect(groups[0].chats.map((item) => item.id)).toEqual(['one', 'two']);
  });

  it('groups by the local calendar day of the sort field', () => {
    const weekday = new Date(2026, 8, 29, 12, 0, 0);
    const older = new Date(2026, 8, 22, 12, 0, 0);
    const lastYear = new Date(2025, 2, 3, 12, 0, 0);
    const chats = [
      chat({ id: 'today', title: 'Today', updated_at: iso(2026, 9, 2, 10) }),
      chat({ id: 'yesterday', title: 'Yesterday', updated_at: iso(2026, 9, 1, 22) }),
      chat({ id: 'week', title: 'Weekday', updated_at: weekday.toISOString() }),
      chat({ id: 'older', title: 'Older', updated_at: older.toISOString() }),
      chat({ id: 'year', title: 'Last year', updated_at: lastYear.toISOString() }),
    ];

    const groups = arrangeChats(chats, { groupBy: 'date', sort: 'updated_desc', now });

    expect(groups.map((group) => group.label)).toEqual([
      'Today',
      'Yesterday',
      weekday.toLocaleDateString([], { weekday: 'short' }),
      older.toLocaleDateString([], { month: 'short', day: 'numeric' }),
      lastYear.toLocaleDateString([], { month: 'short', day: 'numeric', year: 'numeric' }),
    ]);
    expect(groups.map((group) => group.chats.map((item) => item.id))).toEqual([
      ['today'],
      ['yesterday'],
      ['week'],
      ['older'],
      ['year'],
    ]);
  });

  it('uses created_at for date groups when sorting by created date', () => {
    const chats = [
      chat({
        id: 'created-today',
        title: 'Created today',
        created_at: iso(2026, 9, 2),
        updated_at: iso(2025, 0, 1),
      }),
      chat({
        id: 'updated-today',
        title: 'Updated today',
        created_at: iso(2025, 0, 1),
        updated_at: iso(2026, 9, 2),
      }),
    ];

    const byCreated = arrangeChats(chats, { groupBy: 'date', sort: 'created_desc', now });
    expect(byCreated.map((group) => group.label)).toEqual([
      'Today',
      new Date(2025, 0, 1).toLocaleDateString([], {
        month: 'short',
        day: 'numeric',
        year: 'numeric',
      }),
    ]);
    expect(byCreated[0].chats.map((item) => item.id)).toEqual(['created-today']);

    const byUpdated = arrangeChats(chats, { groupBy: 'date', sort: 'updated_desc', now });
    expect(byUpdated[0].chats.map((item) => item.id)).toEqual(['updated-today']);
  });

  it('puts older date groups first when sorting ascending by time', () => {
    const chats = [
      chat({ id: 'today', title: 'Today', updated_at: iso(2026, 9, 2) }),
      chat({ id: 'yesterday', title: 'Yesterday', updated_at: iso(2026, 9, 1) }),
    ];

    expect(
      arrangeChats(chats, { groupBy: 'date', sort: 'updated_asc', now }).map((group) => group.label)
    ).toEqual(['Yesterday', 'Today']);
  });

  it('keeps newest date groups first when sorting by title', () => {
    const chats = [
      chat({ id: 'b', title: 'Beta', updated_at: iso(2026, 9, 2) }),
      chat({ id: 'a', title: 'Alpha', updated_at: iso(2026, 9, 2) }),
      chat({ id: 'old', title: 'Old', updated_at: iso(2026, 9, 1) }),
    ];

    const groups = arrangeChats(chats, { groupBy: 'date', sort: 'title_asc', now });
    expect(groups.map((group) => group.label)).toEqual(['Today', 'Yesterday']);
    expect(groups[0].chats.map((item) => item.id)).toEqual(['a', 'b']);
  });

  it('does not mutate the input list', () => {
    const chats = [
      chat({ id: 'b', title: 'B', updated_at: iso(2026, 8, 1) }),
      chat({ id: 'a', title: 'A', updated_at: iso(2026, 9, 2) }),
    ];
    const snapshot = [...chats];
    arrangeChats(chats, { groupBy: 'none', sort: 'title_asc' });
    expect(chats).toEqual(snapshot);
  });
});

describe('chat arrange storage', () => {
  beforeEach(() => {
    localStorage.removeItem(CHAT_GROUP_STORAGE_KEY);
    localStorage.removeItem(CHAT_SORT_STORAGE_KEY);
  });

  it('defaults and round-trips valid values', () => {
    expect(readChatGroupBy()).toBe('none');
    expect(readChatSort()).toBe('updated_desc');
    writeChatGroupBy('project');
    writeChatSort('title_asc');
    expect(readChatGroupBy()).toBe('project');
    expect(readChatSort()).toBe('title_asc');
  });

  it('falls back when stored values are not options', () => {
    localStorage.setItem(CHAT_GROUP_STORAGE_KEY, 'folder');
    localStorage.setItem(CHAT_SORT_STORAGE_KEY, 'popular');
    expect(readChatGroupBy()).toBe('none');
    expect(readChatSort()).toBe('updated_desc');
  });
});

function ids(groups: ReturnType<typeof arrangeChats>): string[] {
  return groups.flatMap((group) => group.chats.map((item) => item.id));
}
