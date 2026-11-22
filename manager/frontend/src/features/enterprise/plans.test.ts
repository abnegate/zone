import { describe, expect, it } from 'bun:test';
import { availableOn, editionOf, includes, planLabel } from './plans';
import { EDITIONS, Edition, PLANS, Plan } from './schemas';
import type { Edition as EditionValue, Plan as PlanValue } from './types';

const INCLUDED: readonly (readonly [EditionValue, PlanValue, boolean])[] = [
  [Edition.Community, Plan.Team, false],
  [Edition.Community, Plan.Enterprise, false],
  [Edition.Team, Plan.Team, true],
  [Edition.Team, Plan.Enterprise, false],
  [Edition.Enterprise, Plan.Team, true],
  [Edition.Enterprise, Plan.Enterprise, true],
];

describe('includes', () => {
  for (const [edition, plan, included] of INCLUDED) {
    it(`${included ? 'gives' : 'withholds'} ${plan} features ${included ? 'to' : 'from'} ${edition}`, () => {
      expect(includes(edition, plan)).toBe(included);
    });
  }

  it('covers every edition against every plan', () => {
    expect(INCLUDED).toHaveLength(EDITIONS.length * PLANS.length);
  });
});

describe('editionOf', () => {
  it('names the edition a license on each plan grants', () => {
    expect(editionOf(Plan.Team)).toBe(Edition.Team);
    expect(editionOf(Plan.Enterprise)).toBe(Edition.Enterprise);
  });

  it('grants every feature of the plan it maps from', () => {
    for (const plan of PLANS) {
      expect(includes(editionOf(plan), plan)).toBe(true);
    }
  });
});

describe('planLabel', () => {
  it('names each plan', () => {
    expect(planLabel(Plan.Team)).toBe('Team');
    expect(planLabel(Plan.Enterprise)).toBe('Enterprise');
  });
});

describe('availableOn', () => {
  it('says which plan a feature is available on', () => {
    expect(availableOn(Plan.Team)).toBe('Available on Team');
    expect(availableOn(Plan.Enterprise)).toBe('Available on Enterprise');
  });
});
