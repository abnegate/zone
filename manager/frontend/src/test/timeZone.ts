import { afterAll, beforeAll } from 'bun:test';

export function restoreTimeZone(previous: string | undefined): void {
  // Bun keeps the last zone when TZ is assigned undefined; only deleting it resets the clock.
  if (previous === undefined) delete process.env.TZ;
  else process.env.TZ = previous;
}

export function pinTimeZone(zone: string): void {
  const previous = process.env.TZ;

  beforeAll(() => {
    process.env.TZ = zone;
  });

  afterAll(() => {
    restoreTimeZone(previous);
  });
}
