import { describe, expect, it } from 'bun:test';
import { LICENSE_PATH } from './licensePath';

describe('LICENSE_PATH', () => {
  it('points at the server License settings', () => {
    expect(LICENSE_PATH).toBe('/settings/server/license');
  });
});
