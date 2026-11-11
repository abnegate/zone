import type { ReactNode } from 'react';
import type { StageId } from '../../status';

const CENTRE = 7;
const RING = 6;
const CORE = 3.5;
const STROKE = 1.5;

const ring = (
  <circle
    className="status-icon-ring"
    cx={CENTRE}
    cy={CENTRE}
    r={RING}
    fill="none"
    strokeWidth={STROKE}
  />
);

const disc = <circle className="status-icon-disc" cx={CENTRE} cy={CENTRE} r={RING + STROKE / 2} />;

function wedge(sweep: 'half' | 'threeQuarters'): string {
  const end =
    sweep === 'half' ? `0 1 ${CENTRE} ${CENTRE + CORE}` : `1 1 ${CENTRE - CORE} ${CENTRE}`;
  return `M${CENTRE} ${CENTRE}V${CENTRE - CORE}A${CORE} ${CORE} 0 ${end}Z`;
}

const GLYPHS: Record<StageId, ReactNode> = {
  planned: (
    <circle
      className="status-icon-ring"
      cx={CENTRE}
      cy={CENTRE}
      r={RING}
      fill="none"
      strokeWidth={STROKE}
      strokeDasharray="2.36 2.36"
    />
  ),
  in_progress: (
    <>
      {ring}
      <path className="status-icon-fill" d={wedge('half')} />
    </>
  ),
  blocked: (
    <>
      {disc}
      <path
        className="status-icon-mark"
        d="M4.25 7h5.5"
        fill="none"
        strokeWidth={STROKE}
        strokeLinecap="round"
      />
    </>
  ),
  paused: (
    <>
      {ring}
      <path
        className="status-icon-fill"
        d="M5.75 5v4M8.25 5v4"
        fill="none"
        strokeWidth={STROKE}
        strokeLinecap="round"
      />
    </>
  ),
  review: (
    <>
      {ring}
      <path className="status-icon-fill" d={wedge('threeQuarters')} />
    </>
  ),
  complete: (
    <>
      {disc}
      <path
        className="status-icon-mark"
        d="M4.5 7.25l1.75 1.75 3.25-3.5"
        fill="none"
        strokeWidth={STROKE}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </>
  ),
  cancelled: (
    <>
      {disc}
      <path
        className="status-icon-mark"
        d="M5 5l4 4M9 5l-4 4"
        fill="none"
        strokeWidth={STROKE}
        strokeLinecap="round"
      />
    </>
  ),
};

export function StatusIcon({ stage }: { stage: StageId }) {
  return (
    <svg
      aria-hidden="true"
      focusable="false"
      className={`status-icon status-icon-${stage}`}
      viewBox="0 0 14 14"
      width="14"
      height="14"
    >
      {GLYPHS[stage]}
    </svg>
  );
}
