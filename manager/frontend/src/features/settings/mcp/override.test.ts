import { describe, expect, it } from 'bun:test';
import { OVERRIDE_LABELS, OVERRIDES, Override, overrideOf, overrideValue } from './override';

describe('workspace overrides', () => {
  it('reads the override from the workspace setting', () => {
    expect(overrideOf({ workspace_enabled: null })).toBe(Override.Inherit);
    expect(overrideOf({ workspace_enabled: true })).toBe(Override.On);
    expect(overrideOf({ workspace_enabled: false })).toBe(Override.Off);
  });

  it('writes the workspace setting from the override, null clearing it', () => {
    expect(overrideValue(Override.Inherit)).toBeNull();
    expect(overrideValue(Override.On)).toBe(true);
    expect(overrideValue(Override.Off)).toBe(false);
  });

  it('round trips every override', () => {
    expect(
      OVERRIDES.map((override) => overrideOf({ workspace_enabled: overrideValue(override) }))
    ).toEqual([Override.Inherit, Override.On, Override.Off]);
  });

  it('labels the overrides in display order', () => {
    expect(OVERRIDES.map((override) => OVERRIDE_LABELS[override])).toEqual([
      'Inherit',
      'On',
      'Off',
    ]);
  });
});
