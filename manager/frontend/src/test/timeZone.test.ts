import { afterEach, describe, expect, it } from 'bun:test';
import { restoreTimeZone } from './timeZone';

const EPOCH = new Date(0);
const original = process.env.TZ;

function clock(): string {
  return EPOCH.toLocaleTimeString('en-US');
}

describe('restoreTimeZone', () => {
  afterEach(() => {
    restoreTimeZone(original);
  });

  it('returns the clock to the process zone when no zone was set before', () => {
    delete process.env.TZ;
    const unset = clock();

    process.env.TZ = 'Pacific/Kiritimati';
    expect(clock()).not.toBe(unset);

    restoreTimeZone(undefined);
    expect(process.env.TZ).toBeUndefined();
    expect(clock()).toBe(unset);
  });

  it('puts back a zone that was set before', () => {
    process.env.TZ = 'Pacific/Kiritimati';
    restoreTimeZone('UTC');

    expect(process.env.TZ).toBe('UTC');
    expect(clock()).toBe('12:00:00 AM');
  });
});
