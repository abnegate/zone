import { describe, expect, it } from 'bun:test';
import { analyticsKeys } from './queryKeys';

describe('analytics query keys', () => {
  it('keys an overview on its workspace and period', () => {
    expect(analyticsKeys.overview('workspace-1', 'week')).toEqual([
      'analytics',
      'workspace-1',
      'week',
    ]);
  });

  it('keeps every overview under its workspace', () => {
    const workspace = analyticsKeys.workspace('workspace-1');
    expect(workspace).toEqual(['analytics', 'workspace-1']);
    expect(analyticsKeys.overview('workspace-1', 'quarter').slice(0, 2)).toEqual([...workspace]);
  });

  it('keys each period apart', () => {
    expect(analyticsKeys.overview('workspace-1', 'week')).not.toEqual(
      analyticsKeys.overview('workspace-1', 'month')
    );
    expect(analyticsKeys.overview('workspace-1', 'week')).not.toEqual(
      analyticsKeys.overview('workspace-2', 'week')
    );
  });
});
