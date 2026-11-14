import { describe, expect, it } from 'bun:test';
import { Edits, ProjectEdit } from './Edits';

describe('Edits', () => {
  it('starts untouched with an empty request', () => {
    expect(Edits.NONE.touched).toBe(false);
    expect(Edits.NONE.request()).toEqual({});
  });

  it('requests only the keys that were set', () => {
    const edits = Edits.NONE.with('status', 'paused').with('is_agentic', false);

    expect(edits.touched).toBe(true);
    expect(edits.request()).toEqual({ status: 'paused', is_agentic: false });
    expect(Object.keys(edits.request())).toEqual(['status', 'is_agentic']);
  });

  it('removes a key set back to undefined', () => {
    const edits = Edits.NONE.with('priority', 5).with('priority', undefined);

    expect(edits.touched).toBe(false);
    expect(edits.request()).toEqual({});
    expect('priority' in edits.request()).toBe(false);
  });

  it('replaces a key set twice', () => {
    expect(Edits.NONE.with('priority', 2).with('priority', 4).request()).toEqual({ priority: 4 });
  });

  it('leaves the original untouched', () => {
    const original = Edits.NONE.with('status', 'blocked');
    const next = original.with('require_plan_approval', true);

    expect(original.request()).toEqual({ status: 'blocked' });
    expect(next.request()).toEqual({ status: 'blocked', require_plan_approval: true });
    expect(Edits.NONE.request()).toEqual({});
  });

  it('treats every project as unchanged until one is edited', () => {
    expect(Edits.NONE.projectEdit('project-1')).toBe(ProjectEdit.UNCHANGED);
  });

  it('adds a project without naming any other', () => {
    const edits = Edits.NONE.project('project-3', ProjectEdit.ADD);

    expect(edits.touched).toBe(true);
    expect(edits.projectEdit('project-3')).toBe(ProjectEdit.ADD);
    expect(edits.request()).toEqual({ add_project_ids: ['project-3'] });
  });

  it('removes a project without naming any other', () => {
    const edits = Edits.NONE.project('project-1', ProjectEdit.REMOVE);

    expect(edits.projectEdit('project-1')).toBe(ProjectEdit.REMOVE);
    expect(edits.request()).toEqual({ remove_project_ids: ['project-1'] });
  });

  it('adds and removes different projects in one request', () => {
    const edits = Edits.NONE.with('priority', 3)
      .project('project-1', ProjectEdit.REMOVE)
      .project('project-2', ProjectEdit.ADD)
      .project('project-3', ProjectEdit.ADD);

    expect(edits.request()).toEqual({
      priority: 3,
      add_project_ids: ['project-2', 'project-3'],
      remove_project_ids: ['project-1'],
    });
  });

  it('never lists a project as both added and removed', () => {
    const edits = Edits.NONE.project('project-2', ProjectEdit.ADD).project(
      'project-2',
      ProjectEdit.REMOVE
    );

    expect(edits.projectEdit('project-2')).toBe(ProjectEdit.REMOVE);
    expect(edits.request()).toEqual({ remove_project_ids: ['project-2'] });
  });

  it('forgets a project set back to unchanged', () => {
    const edits = Edits.NONE.project('project-3', ProjectEdit.ADD).project(
      'project-3',
      ProjectEdit.UNCHANGED
    );

    expect(edits.touched).toBe(false);
    expect(edits.projectEdit('project-3')).toBe(ProjectEdit.UNCHANGED);
    expect(edits.request()).toEqual({});
    expect('add_project_ids' in edits.request()).toBe(false);
    expect('remove_project_ids' in edits.request()).toBe(false);
  });

  it('leaves the original project edits untouched', () => {
    const original = Edits.NONE.project('project-1', ProjectEdit.ADD);
    original.project('project-1', ProjectEdit.REMOVE).project('project-2', ProjectEdit.ADD);

    expect(original.request()).toEqual({ add_project_ids: ['project-1'] });
    expect(Edits.NONE.projectEdit('project-1')).toBe(ProjectEdit.UNCHANGED);
  });

  it('hands out a fresh request each time', () => {
    const edits = Edits.NONE.project('project-1', ProjectEdit.ADD);
    const request = edits.request();
    request.status = 'complete';
    request.add_project_ids?.push('project-9');

    expect(edits.request()).toEqual({ add_project_ids: ['project-1'] });
    expect(edits.request()).not.toBe(edits.request());
  });
});
