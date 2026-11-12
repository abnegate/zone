import { describe, expect, it } from 'bun:test';
import { STATUS_BADGES } from './status';

describe('STATUS_BADGES', () => {
  it('maps every status to its badge', () => {
    expect(STATUS_BADGES).toEqual({
      connected: { variant: 'success', label: 'Connected' },
      failed: { variant: 'destructive', label: 'Failed' },
      connecting: { variant: 'neutral', label: 'Connecting' },
      disabled: { variant: 'outline', label: 'Disabled' },
    });
  });
});
