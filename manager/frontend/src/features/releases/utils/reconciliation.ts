import { shortSha } from '../../github';
import type { Reconciliation } from '../types';

export const RECONCILIATION_NEEDED =
  'The release was only partly created. Reconcile it on GitHub before publishing again. Nothing was deleted.';

const MARKER_PREFIX = 'zone-release:';

function reference({ tag, reference_created: created }: Reconciliation): string {
  if (created === true) return `refs/tags/${tag} points at it.`;
  if (created === false) return `refs/tags/${tag} was not created.`;
  return `Whether refs/tags/${tag} points at it could not be read.`;
}

function release({ tag, release_id: id, release_exists: exists }: Reconciliation): string {
  if (id !== null) return `Draft release ${id} exists but was not confirmed published.`;
  if (exists) return `GitHub already holds a release for ${tag}.`;
  return 'No release was confirmed created.';
}

export function reconciliationMarker(reconciliation: Reconciliation): string {
  return `${MARKER_PREFIX}${reconciliation.marker}`;
}

export function reconciliationFacts(reconciliation: Reconciliation): string[] {
  return [
    `Tag object ${shortSha(reconciliation.tag_object)} was written for ${reconciliation.tag}.`,
    reference(reconciliation),
    release(reconciliation),
    `Search GitHub for ${reconciliationMarker(reconciliation)} to find what was written.`,
  ];
}
