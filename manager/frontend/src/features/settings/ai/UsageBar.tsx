import type { UsageWindow } from './schemas';
import { countsOf, filledOf, percentOf, resetsIn, severityOf } from './usage';

interface UsageBarProps {
  window: UsageWindow;
  now: number;
}

export function UsageBar({ window, now }: UsageBarProps) {
  const percent = percentOf(window);
  const counts = countsOf(window);
  const resets = resetsIn(window.resets_at, now);
  const filled = filledOf(window);
  const used = window.used_percent === null ? 'usage unknown' : `${percent} used`;
  const reading = [used, counts, resets].filter(Boolean).join(', ');

  return (
    <div className="usage-bar" data-severity={severityOf(window.used_percent)}>
      <span className="usage-bar-name">{window.name}</span>
      <div
        className="usage-bar-track"
        role="meter"
        aria-label={`${window.name} usage`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={filled}
        aria-valuetext={reading}
      >
        <div className="usage-bar-fill" style={{ width: `${filled}%` }} />
      </div>
      <span className="usage-bar-percent">{percent}</span>
      <span className="usage-bar-counts">{counts}</span>
      <span className="usage-bar-resets">{resets}</span>
    </div>
  );
}
