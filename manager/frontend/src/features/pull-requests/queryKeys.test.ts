import { describe, expect, it } from 'bun:test';
import { HEAD, SOURCE_ID } from '../../test/pullRequests';
import { PULL_REQUESTS_KEY, pullRequestKeys } from './queryKeys';

const revision = { source_id: SOURCE_ID, number: 42, head: HEAD };

function startsWith(key: readonly unknown[], prefix: readonly unknown[]): boolean {
  return prefix.every((segment, index) => key[index] === segment);
}

describe('pull request query keys', () => {
  it('scopes every key to the workspace', () => {
    const workspace = pullRequestKeys.workspace('workspace-1');
    expect(workspace).toEqual([PULL_REQUESTS_KEY, 'workspace-1']);
    for (const key of [
      pullRequestKeys.list('workspace-1'),
      pullRequestKeys.detail('workspace-1', revision),
      pullRequestKeys.diff('workspace-1', revision),
      pullRequestKeys.commits('workspace-1', revision),
      pullRequestKeys.commit('workspace-1', revision, 'c'.repeat(40)),
      pullRequestKeys.log('workspace-1', revision, 9001),
    ]) {
      expect(startsWith(key, workspace)).toBe(true);
    }
  });

  it('keys the list apart from any one pull request', () => {
    expect(pullRequestKeys.list('workspace-1')).toEqual([PULL_REQUESTS_KEY, 'workspace-1', 'list']);
  });

  it('keys what was read from a pull request on its head commit', () => {
    expect(pullRequestKeys.diff('workspace-1', revision)).toEqual([
      PULL_REQUESTS_KEY,
      'workspace-1',
      'diff',
      SOURCE_ID,
      42,
      HEAD,
    ]);
    expect(pullRequestKeys.diff('workspace-1', revision)).not.toEqual(
      pullRequestKeys.diff('workspace-1', { ...revision, head: 'd'.repeat(40) })
    );
  });

  it('keys a commit diff on its commit and a log on its job', () => {
    expect(pullRequestKeys.commit('workspace-1', revision, 'c'.repeat(40)).at(-1)).toBe(
      'c'.repeat(40)
    );
    expect(pullRequestKeys.log('workspace-1', revision, 9001).at(-1)).toBe(9001);
  });

  it('keys nothing selected without a pull request', () => {
    expect(pullRequestKeys.detail('workspace-1', null)).toEqual([
      PULL_REQUESTS_KEY,
      'workspace-1',
      'detail',
      null,
      null,
      null,
    ]);
  });
});
