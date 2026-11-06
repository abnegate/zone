import { beforeEach, describe, expect, it } from 'bun:test';
import type { Project } from '../types';
import {
  applyOrder,
  DEFAULT_PROJECT_SORT,
  moveId,
  readSort,
  sortKey,
  sortProjects,
  writeSort,
} from './order';

function project(id: string, overrides: Partial<Project> = {}): Project {
  return {
    id,
    name: id,
    description: null,
    status: 'active',
    github_repo_url: null,
    source_id: null,
    auto: false,
    position: 0,
    last_used_at: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

const zone = project('zone', {
  name: 'Zone',
  position: 3,
  last_used_at: '2026-11-04T10:00:00Z',
  created_at: '2026-10-03T00:00:00Z',
});
const cloud = project('cloud', {
  name: 'cloud',
  position: 1,
  last_used_at: null,
  created_at: '2026-11-06T00:00:00Z',
});
const jbs = project('jbs', {
  name: 'JBS',
  status: 'on_hold',
  position: 2,
  last_used_at: '2026-11-05T10:00:00Z',
  created_at: '2026-11-05T00:00:00Z',
});
const projects = [zone, cloud, jbs];

function ids(list: readonly Project[]): string[] {
  return list.map((item) => item.id);
}

describe('sortProjects', () => {
  it('keeps the custom order the workspace dragged them into', () => {
    expect(ids(sortProjects(projects, 'custom'))).toEqual(['cloud', 'jbs', 'zone']);
  });

  it('puts the most recently used first and the never used last', () => {
    expect(ids(sortProjects(projects, 'recent'))).toEqual(['jbs', 'zone', 'cloud']);
  });

  it('puts the newest first by date added', () => {
    expect(ids(sortProjects(projects, 'added'))).toEqual(['cloud', 'jbs', 'zone']);
  });

  it('sorts by name without regard to case', () => {
    expect(ids(sortProjects(projects, 'name'))).toEqual(['cloud', 'jbs', 'zone']);
  });

  it('puts active projects first, then keeps the custom order', () => {
    expect(ids(sortProjects(projects, 'status'))).toEqual(['cloud', 'zone', 'jbs']);
  });

  it('leaves the list it was given alone', () => {
    sortProjects(projects, 'name');

    expect(ids(projects)).toEqual(['zone', 'cloud', 'jbs']);
  });
});

describe('moveId', () => {
  it('moves an id to the index it was dropped at', () => {
    expect(moveId(['a', 'b', 'c'], 'c', 0)).toEqual(['c', 'a', 'b']);
    expect(moveId(['a', 'b', 'c'], 'a', 2)).toEqual(['b', 'c', 'a']);
  });

  it('leaves the list as it was for an id it does not hold', () => {
    expect(moveId(['a', 'b'], 'z', 0)).toEqual(['a', 'b']);
  });
});

describe('applyOrder', () => {
  it('puts the listed projects in the places they held between them', () => {
    const arranged = applyOrder(projects, ['zone', 'cloud']);

    expect(ids(arranged)).toEqual(['zone', 'jbs', 'cloud']);
    expect(arranged.map((item) => item.position)).toEqual([1, 2, 3]);
  });

  it('moves projects that share a position, as an old server lists them', () => {
    const tied = [project('a', { position: 0 }), project('b', { position: 0 })];

    const arranged = applyOrder(tied, ['b', 'a']);

    expect(ids(arranged)).toEqual(['b', 'a']);
    expect(ids(sortProjects(arranged, 'custom'))).toEqual(['b', 'a']);
  });
});

describe('the saved sort', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('is custom until a viewer picks another', () => {
    expect(readSort(sortKey('user-1'))).toBe(DEFAULT_PROJECT_SORT);
    expect(DEFAULT_PROJECT_SORT).toBe('custom');
  });

  it('is kept for each viewer apart', () => {
    writeSort(sortKey('user-1'), 'recent');

    expect(readSort(sortKey('user-1'))).toBe('recent');
    expect(readSort(sortKey('user-2'))).toBe('custom');
  });

  it('falls back to custom when what was stored is not a sort', () => {
    localStorage.setItem(sortKey('user-1'), 'sideways');

    expect(readSort(sortKey('user-1'))).toBe('custom');
  });
});
