const STEPS = [1, 2, 5, 10] as const;

const BASE = 10;

export function niceCeiling(value: number): number {
  if (value <= 1) return 1;
  const magnitude = BASE ** Math.floor(Math.log10(value));
  const step = STEPS.find((candidate) => candidate * magnitude >= value) ?? BASE;
  return step * magnitude;
}

export function proportion(value: number, maximum: number): number {
  if (maximum <= 0) return 0;
  return Math.min(1, Math.max(0, value / maximum));
}

export function labelEvery(points: number, labels: number): number {
  return Math.max(1, Math.ceil(points / labels));
}
