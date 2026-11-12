import { describe, expect, it } from 'bun:test';
import { Edits } from './Edits';

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

  it('keeps an empty project list as a change', () => {
    const edits = Edits.NONE.with('project_ids', []);

    expect(edits.touched).toBe(true);
    expect(edits.request()).toEqual({ project_ids: [] });
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

  it('hands out a fresh request each time', () => {
    const edits = Edits.NONE.with('project_ids', ['project-1']);
    const request = edits.request();
    request.status = 'complete';

    expect(edits.request()).toEqual({ project_ids: ['project-1'] });
    expect(edits.request()).not.toBe(edits.request());
  });
});
