import { describe, expect, it } from 'bun:test';
import type { CheckDetail } from '../types';
import { failedChecks } from './checks';

function check(changes: Partial<CheckDetail>): CheckDetail {
  return {
    kind: 'check_run',
    id: 9001,
    name: 'test',
    state: 'failure',
    url: 'https://github.com/acme/zone/actions/runs/1/job/9001',
    updated_at: '2026-11-13T09:58:00Z',
    ...changes,
  };
}

describe('failedChecks', () => {
  it('keeps only the checks that failed, in order', () => {
    const failed = failedChecks({
      check_details: [
        check({ id: 1, name: 'lint', state: 'success' }),
        check({ id: 2, name: 'test' }),
        check({ id: 3, name: 'build', state: 'pending' }),
        check({ id: 4, name: 'e2e' }),
      ],
    });
    expect(failed.map((each) => each.name)).toEqual(['test', 'e2e']);
  });

  it('carries the Actions job of a failed check run so its log can be read', () => {
    expect(failedChecks({ check_details: [check({ id: 9001 })] })).toEqual([
      {
        name: 'test',
        state: 'failure',
        url: 'https://github.com/acme/zone/actions/runs/1/job/9001',
        jobId: 9001,
      },
    ]);
  });

  it.each(['workflow', 'status', 'unknown'] as const)('has no job for a failed %s', (kind) => {
    const [failed] = failedChecks({ check_details: [check({ kind, id: null })] });
    expect(failed.jobId).toBeNull();
  });

  it('has no job for a check run that is not an Actions job', () => {
    const [failed] = failedChecks({
      check_details: [check({ id: null, url: 'https://ci.example.com/build/7' })],
    });
    expect(failed.jobId).toBeNull();
  });

  it('keeps a failed check without a link', () => {
    const [failed] = failedChecks({ check_details: [check({ url: null })] });
    expect(failed.url).toBeNull();
  });

  it('finds nothing when no check failed', () => {
    expect(failedChecks({ check_details: [check({ state: 'success' })] })).toEqual([]);
    expect(failedChecks({ check_details: [] })).toEqual([]);
  });
});
