import { afterEach, describe, expect, it, spyOn } from 'bun:test';
import { TIME_ZONE_HEADER, timeZoneHeaders } from './timeZone';

describe('timeZoneHeaders', () => {
  afterEach(() => {
    (Intl.DateTimeFormat as unknown as { mockRestore?: () => void }).mockRestore?.();
  });

  it("reports the browser's IANA zone", () => {
    expect(timeZoneHeaders()).toEqual({
      [TIME_ZONE_HEADER]: Intl.DateTimeFormat().resolvedOptions().timeZone,
    });
  });

  it('sends nothing when the browser cannot name its zone', () => {
    spyOn(Intl, 'DateTimeFormat').mockImplementation((() => {
      throw new Error('no Intl');
    }) as unknown as typeof Intl.DateTimeFormat);
    expect(timeZoneHeaders()).toEqual({});
  });
});
