import type { Tone } from '../Tone';

const PERCENT = 100;

function filled(value: number, maximum: number): number {
  if (maximum <= 0) return 0;
  return Math.min(1, Math.max(0, value / maximum)) * PERCENT;
}

export interface MeterRowProps {
  label: string;
  value: number;
  maximum: number;
  display: string;
  tone: Tone;
  detail?: string;
}

export function MeterRow({ label, value, maximum, display, tone, detail }: MeterRowProps) {
  return (
    <li className="analytics-meter" data-tone={tone}>
      <span className="analytics-meter-label">{label}</span>
      <span className="analytics-meter-track" aria-hidden="true">
        <span
          className="analytics-meter-fill"
          style={{ inlineSize: `${filled(value, maximum)}%` }}
        />
      </span>
      <span className="analytics-meter-value">{display}</span>
      {detail && <span className="analytics-meter-detail">{detail}</span>}
    </li>
  );
}
