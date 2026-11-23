import { describe, expect, it } from 'bun:test';
import { ORGANIZATION_ID, ORGANIZATION_LABELS, WORKSPACE_ID, WORKSPACE_LABELS } from './fixtures';
import {
  fromOrganization,
  fromWorkspace,
  organizationPlacement,
  PlacementKind,
  parseLabels,
  sameLabels,
  validLabel,
  withLabels,
  withoutLabel,
  workspacePlacement,
} from './placement';
import { fleetQueryKeys } from './queryKeys';

describe('runner placement', () => {
  it('reads labels as the server parses a list', () => {
    expect(parseLabels(' gpu, region=eu,gpu ')).toEqual(['gpu', 'region=eu']);
    expect(parseLabels('gpu region=eu')).toEqual(['gpu', 'region=eu']);
    expect(parseLabels('  ,  ')).toEqual([]);
  });

  it('accepts the labels the server accepts and refuses the rest', () => {
    for (const label of ['gpu', 'region=eu', 'agents=claude', 'zone.io/tier_1', '9x']) {
      expect(validLabel(label), label).toBe(true);
    }
    for (const label of ['GPU', '-x', 'a b', '', 'a'.repeat(64), 'é']) {
      expect(validLabel(label), label).toBe(false);
    }
    expect(validLabel('a'.repeat(63))).toBe(true);
  });

  it('keeps label sets sorted and free of repeats', () => {
    expect(withLabels(['region=eu'], ['gpu', 'region=eu'])).toEqual(['gpu', 'region=eu']);
    expect(withoutLabel(['gpu', 'region=eu'], 'gpu')).toEqual(['region=eu']);
    expect(sameLabels(['b', 'a'], ['a', 'b', 'a'])).toBe(true);
    expect(sameLabels(['a'], ['a', 'b'])).toBe(false);
  });

  it('reads an organization as requiring its own labels, with nothing inherited', () => {
    const placement = fromOrganization(ORGANIZATION_LABELS);

    expect(placement.inherited).toEqual([]);
    expect(placement.own).toEqual(ORGANIZATION_LABELS.labels);
    expect(placement.effective).toEqual(ORGANIZATION_LABELS.labels);
  });

  it('reads a workspace as inheriting its organization labels', () => {
    const placement = fromWorkspace(WORKSPACE_LABELS);

    expect(placement.inherited).toEqual(WORKSPACE_LABELS.organization);
    expect(placement.own).toEqual(WORKSPACE_LABELS.workspace);
    expect(placement.effective).toEqual(WORKSPACE_LABELS.effective);
  });

  it('keys each scope under the fleet labels, workspaces below their organization', () => {
    expect(fleetQueryKeys.placement(organizationPlacement(ORGANIZATION_ID))).toEqual([
      'fleet',
      'labels',
      PlacementKind.Organization,
      ORGANIZATION_ID,
    ]);
    expect(fleetQueryKeys.placement(workspacePlacement(ORGANIZATION_ID, WORKSPACE_ID))).toEqual([
      ...fleetQueryKeys.workspaces(ORGANIZATION_ID),
      WORKSPACE_ID,
    ]);
    expect(fleetQueryKeys.workspaces(ORGANIZATION_ID).slice(0, 2)).toEqual([
      ...fleetQueryKeys.labels,
    ]);
  });
});
