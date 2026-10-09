import { beforeEach, describe, expect, it } from 'bun:test';
import { isSetupComplete, markSetupComplete, setupCompleteKey } from './setupComplete';

describe('setupComplete', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('keys completion per user', () => {
    expect(setupCompleteKey('user-1')).toBe('manager_setup_complete:user-1');
    expect(isSetupComplete('user-1')).toBe(false);
    markSetupComplete('user-1');
    expect(isSetupComplete('user-1')).toBe(true);
    expect(isSetupComplete('user-2')).toBe(false);
  });
});
