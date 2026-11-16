import { count } from '../format';
import type { Tone } from '../labels';
import type { Tally } from '../types';
import { MeterRow } from './MeterRow';

export interface TallyBarsProps {
  label: string;
  tallies: readonly Tally[];
  labelOf: (name: string) => string;
  toneOf: (name: string) => Tone;
}

export function TallyBars({ label, tallies, labelOf, toneOf }: TallyBarsProps) {
  const maximum = Math.max(0, ...tallies.map((tally) => tally.count));
  return (
    <div className="analytics-tally">
      <h3 className="analytics-subheading">{label}</h3>
      <ul className="analytics-meters" aria-label={label}>
        {tallies.map((tally) => (
          <MeterRow
            key={tally.name}
            label={labelOf(tally.name)}
            value={tally.count}
            maximum={maximum}
            display={count(tally.count)}
            tone={toneOf(tally.name)}
          />
        ))}
      </ul>
    </div>
  );
}
