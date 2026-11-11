import { describe, expect, it } from 'bun:test';
import { relate, related, relationsOf, UNRELATED } from './relations';
import type { Task } from './types';

function task(id: string, dependencies: string[] = []): Task {
  return {
    id,
    workspace_id: 'workspace-1',
    project_ids: [],
    title: `Task ${id}`,
    description: '',
    acceptance_criteria: null,
    status: 'created',
    priority: null,
    model_name: null,
    dependencies,
    created_at: null,
    updated_at: null,
    started_at: null,
    completed_at: null,
    is_agentic: false,
    require_plan_approval: false,
    github_repo_url: null,
    source_id: null,
    source_ids: [],
    queued_at: null,
    worker_id: null,
    pr_url: null,
    branch_name: null,
    pr_status: null,
    pr_created_at: null,
  };
}

function ids(tasks: readonly Task[]): string[] {
  return tasks.map((item) => item.id);
}

describe('relate', () => {
  const spec = task('spec');
  const build = task('build', ['spec']);
  const ship = task('ship', ['build', 'spec']);
  const relations = relate([spec, build, ship]);

  it('resolves the tasks each task depends on, in the order they are listed', () => {
    expect(ids(relations.get('ship')?.dependencies ?? [])).toEqual(['build', 'spec']);
    expect(ids(relations.get('build')?.dependencies ?? [])).toEqual(['spec']);
    expect(relations.get('spec')?.dependencies).toEqual([]);
  });

  it('indexes the tasks that depend on each task, in board order', () => {
    expect(ids(relations.get('spec')?.dependents ?? [])).toEqual(['build', 'ship']);
    expect(ids(relations.get('build')?.dependents ?? [])).toEqual(['ship']);
    expect(relations.get('ship')?.dependents).toEqual([]);
  });

  it('hands back the loaded task itself, not a copy', () => {
    expect(relations.get('build')?.dependencies[0]).toBe(spec);
    expect(relations.get('spec')?.dependents[0]).toBe(build);
  });

  it('has an entry for every task, related or not', () => {
    const alone = relate([task('alone')]);
    expect(alone.get('alone')).toEqual(UNRELATED);
  });

  it('counts a dependency that is not loaded instead of resolving it', () => {
    const orphaned = relate([spec, task('orphan', ['gone', 'spec', 'deleted'])]);
    const orphan = orphaned.get('orphan');
    expect(ids(orphan?.dependencies ?? [])).toEqual(['spec']);
    expect(orphan?.unknown).toBe(2);
    expect(orphaned.has('gone')).toBe(false);
  });

  it('counts a repeated dependency once', () => {
    const repeated = relate([spec, task('twice', ['spec', 'spec', 'gone', 'gone'])]);
    expect(ids(repeated.get('twice')?.dependencies ?? [])).toEqual(['spec']);
    expect(repeated.get('twice')?.unknown).toBe(1);
    expect(ids(repeated.get('spec')?.dependents ?? [])).toEqual(['twice']);
  });

  it('ignores a task that lists itself', () => {
    const circular = relate([task('loop', ['loop'])]);
    expect(circular.get('loop')).toEqual(UNRELATED);
  });

  it('relates two tasks that depend on each other in both directions', () => {
    const cycle = relate([task('left', ['right']), task('right', ['left'])]);
    expect(ids(cycle.get('left')?.dependencies ?? [])).toEqual(['right']);
    expect(ids(cycle.get('left')?.dependents ?? [])).toEqual(['right']);
  });
});

describe('relationsOf', () => {
  const spec = task('spec');
  const ship = task('ship', ['spec']);

  it('relates a task that is not in the list to the tasks that are', () => {
    const build = task('build', ['spec', 'gone']);
    const relations = relationsOf(build, [spec, task('release', ['build'])]);
    expect(ids(relations.dependencies)).toEqual(['spec']);
    expect(ids(relations.dependents)).toEqual(['release']);
    expect(relations.unknown).toBe(1);
  });

  it('reads the dependencies from the task given, not from its copy in the list', () => {
    const stale = task('ship');
    const relations = relationsOf(ship, [spec, stale]);
    expect(ids(relations.dependencies)).toEqual(['spec']);
  });

  it('counts every dependency as unknown while no task is loaded', () => {
    expect(relationsOf(ship, [])).toEqual({ dependencies: [], dependents: [], unknown: 1 });
  });
});

describe('related', () => {
  it('is false for a task with nothing to show', () => {
    expect(related(UNRELATED)).toBe(false);
  });

  it('is true for a dependency, a dependent or a dependency that is not loaded', () => {
    const spec = task('spec');
    expect(related({ ...UNRELATED, dependencies: [spec] })).toBe(true);
    expect(related({ ...UNRELATED, dependents: [spec] })).toBe(true);
    expect(related({ ...UNRELATED, unknown: 1 })).toBe(true);
  });
});
