import type { Tone } from '../labels';
import { proportion } from '../scale';

const PERCENT = 100;

export interface MeterRowProps {
  label: string;
  value: number;
  maximum: number;
  display: string;
  tone: Tone;
  detail?: string;
}

export function MeterRow({ label, value, maximum, display, tone, detail }: MeterRowProps) {
  const filled = proportion(value, maximum) * PERCENT;
  return (
    <li className="analytics-meter" data-tone={tone}>
      <span className="analytics-meter-label">{label}</span>
      <span className="analytics-meter-track" aria-hidden="true">
        <span className="analytics-meter-fill" style={{ inlineSize: `${filled}%` }} />
      </span>
      <span className="analytics-meter-value">{display}</span>
      {detail && <span className="analytics-meter-detail">{detail}</span>}
    </li>
  );
}
