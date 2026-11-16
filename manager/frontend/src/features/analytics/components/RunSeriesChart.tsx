import { type CSSProperties, useId } from 'react';
import { AnalyticsFormat } from '../AnalyticsFormat';
import { periodLabel } from '../period';
import type { SeriesPoint, TimePeriod } from '../types';
import { RunSeriesAxis } from './RunSeriesAxis';

const HEIGHT = 100;
const BAR_WIDTH = 0.7;
const BAR_OFFSET = (1 - BAR_WIDTH) / 2;
const TICK_LABELS = 7;
const GRID_FRACTIONS = [1, 0.5, 0] as const;

export const NO_SERIES = 'No run fell inside this window to chart.';

export const Outcome = {
  Succeeded: 'succeeded',
  Failed: 'failed',
  Cancelled: 'cancelled',
} as const;

export type Outcome = (typeof Outcome)[keyof typeof Outcome];

const STACK: readonly Outcome[] = [Outcome.Succeeded, Outcome.Failed, Outcome.Cancelled];

const OUTCOME_LABELS: Record<Outcome, string> = {
  succeeded: 'Succeeded',
  failed: 'Failed',
  cancelled: 'Cancelled',
};

interface Segment {
  outcome: Outcome;
  y: number;
  height: number;
}

function segments(point: SeriesPoint, maximum: number): Segment[] {
  let floor = HEIGHT;
  return STACK.filter((outcome) => point[outcome] > 0).map((outcome) => {
    const height = (point[outcome] / maximum) * HEIGHT;
    floor -= height;
    return { outcome, y: floor, height };
  });
}

function summary(point: SeriesPoint, period: TimePeriod): string {
  const label = RunSeriesAxis.bucket(point.window, period);
  const runs = `${AnalyticsFormat.count(point.total)} ${point.total === 1 ? 'run' : 'runs'}`;
  if (point.total === 0) return `${label}: no runs`;
  const breakdown = STACK.map(
    (outcome) => `${AnalyticsFormat.count(point[outcome])} ${outcome}`
  ).join(', ');
  return `${label}: ${runs}, ${breakdown}`;
}

export function chartCaption(period: TimePeriod): string {
  return `Finished runs by outcome over the last ${periodLabel(period)}`;
}

export interface RunSeriesChartProps {
  series: readonly SeriesPoint[];
  period: TimePeriod;
}

export function RunSeriesChart({ series, period }: RunSeriesChartProps) {
  const titleId = useId();
  if (series.length === 0) {
    return <p className="analytics-quiet">{NO_SERIES}</p>;
  }
  const maximum = RunSeriesAxis.ceiling(Math.max(0, ...series.map((point) => point.total)));
  const every = RunSeriesAxis.labelEvery(series.length, TICK_LABELS);
  const caption = chartCaption(period);
  const columns = { '--analytics-chart-points': series.length } as CSSProperties;
  const totals = STACK.map((outcome) => ({
    outcome,
    total: series.reduce((sum, point) => sum + point[outcome], 0),
  }));

  return (
    <figure className="analytics-chart" style={columns}>
      <div className="analytics-chart-plot">
        <div className="analytics-chart-axis" aria-hidden="true">
          {GRID_FRACTIONS.map((fraction) => (
            <span key={fraction}>{AnalyticsFormat.count(maximum * fraction)}</span>
          ))}
        </div>
        <svg
          className="analytics-chart-canvas"
          viewBox={`0 0 ${series.length} ${HEIGHT}`}
          preserveAspectRatio="none"
          role="img"
          aria-labelledby={titleId}
        >
          <title id={titleId}>{caption}</title>
          {GRID_FRACTIONS.map((fraction) => (
            <line
              key={fraction}
              className="analytics-chart-grid"
              x1={0}
              x2={series.length}
              y1={HEIGHT - fraction * HEIGHT}
              y2={HEIGHT - fraction * HEIGHT}
              vectorEffect="non-scaling-stroke"
            />
          ))}
          {series.map((point, index) => (
            <g key={point.window.start} className="analytics-chart-bar">
              <title>{summary(point, period)}</title>
              <rect className="analytics-chart-hit" x={index} y={0} width={1} height={HEIGHT} />
              {segments(point, maximum).map((segment) => (
                <rect
                  key={segment.outcome}
                  className={`analytics-series analytics-series--${segment.outcome}`}
                  x={index + BAR_OFFSET}
                  y={segment.y}
                  width={BAR_WIDTH}
                  height={segment.height}
                  vectorEffect="non-scaling-stroke"
                />
              ))}
            </g>
          ))}
        </svg>
      </div>
      <div className="analytics-chart-ticks" aria-hidden="true">
        {series.map((point, index) => (
          <span key={point.window.start}>
            {index % every === 0 ? RunSeriesAxis.tick(point.window, period) : ''}
          </span>
        ))}
      </div>
      <figcaption className="analytics-legend">
        {totals.map(({ outcome, total }) => (
          <span key={outcome} className="analytics-legend-item">
            <span
              className={`analytics-legend-swatch analytics-series--${outcome}`}
              aria-hidden="true"
            />
            {OUTCOME_LABELS[outcome]}
            <span className="analytics-legend-value">{AnalyticsFormat.count(total)}</span>
          </span>
        ))}
        <span className="analytics-legend-note">{RunSeriesAxis.note(period)}</span>
      </figcaption>
      <div className="sr-only">
        <table>
          <caption>{caption}</caption>
          <thead>
            <tr>
              <th scope="col">Period</th>
              <th scope="col">Runs</th>
              {STACK.map((outcome) => (
                <th key={outcome} scope="col">
                  {OUTCOME_LABELS[outcome]}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {series.map((point) => (
              <tr key={point.window.start}>
                <th scope="row">{RunSeriesAxis.bucket(point.window, period)}</th>
                <td>{AnalyticsFormat.count(point.total)}</td>
                {STACK.map((outcome) => (
                  <td key={outcome}>{AnalyticsFormat.count(point[outcome])}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </figure>
  );
}
