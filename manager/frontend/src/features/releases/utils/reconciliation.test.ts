import { describe, expect, it } from 'bun:test';
import type { Reconciliation } from '../types';
import { reconciliationFacts, reconciliationMarker } from './reconciliation';

const MARKER = '0b9f1c52-6d4e-4a8b-9f2e-1c3d5e7f9a0b';

function reconciliation(overrides: Partial<Reconciliation> = {}): Reconciliation {
  return {
    error: 'The release was only partly created',
    code: 'release_manual_reconciliation_required',
    tag: 'pkg/v1.4.1',
    tag_object: 'c'.repeat(40),
    reference_created: true,
    release_id: null,
    release_exists: false,
    marker: MARKER,
    ...overrides,
  };
}

describe('reconciliationFacts', () => {
  it('names the tag object, the reference, the release and the marker', () => {
    expect(reconciliationFacts(reconciliation({ release_id: 91 }))).toEqual([
      'Tag object ccccccc was written for pkg/v1.4.1.',
      'refs/tags/pkg/v1.4.1 points at it.',
      'Draft release 91 exists but was not confirmed published.',
      `Search GitHub for zone-release:${MARKER} to find what was written.`,
    ]);
  });

  it('says when the reference was not created', () => {
    expect(reconciliationFacts(reconciliation({ reference_created: false }))[1]).toBe(
      'refs/tags/pkg/v1.4.1 was not created.'
    );
  });

  it('says when the reference could not be read back', () => {
    expect(reconciliationFacts(reconciliation({ reference_created: null }))[1]).toBe(
      'Whether refs/tags/pkg/v1.4.1 points at it could not be read.'
    );
  });

  it('says when GitHub already held a release for the tag', () => {
    expect(reconciliationFacts(reconciliation({ release_exists: true }))[2]).toBe(
      'GitHub already holds a release for pkg/v1.4.1.'
    );
  });

  it('says when no release was confirmed', () => {
    expect(reconciliationFacts(reconciliation())[2]).toBe('No release was confirmed created.');
  });
});

describe('reconciliationMarker', () => {
  it('is the marker written into the tag message and the release body', () => {
    expect(reconciliationMarker(reconciliation())).toBe(`zone-release:${MARKER}`);
  });
});
