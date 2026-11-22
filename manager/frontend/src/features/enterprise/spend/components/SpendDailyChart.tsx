import { type CSSProperties, useId } from 'react';
import { RunSeriesAxis } from '../../../analytics/components/RunSeriesAxis';
import { dateLabel, dayLabel } from '../calendar';
import {
  CHART_CAPTION,
  CHART_NOTE,
  COMPLETION_TOKENS_TILE,
  DAY_COLUMN,
  NO_SPEND,
  PROMPT_TOKENS_TILE,
  REQUESTS_COLUMN,
  SPEND_COLUMN,
} from '../labels';
import { axisMoney, count, money } from '../money';
import type { DayUsage } from '../schemas';

const HEIGHT = 100;

const BAR_WIDTH = 0.7;

const BAR_OFFSET = (1 - BAR_WIDTH) / 2;

const TICK_LABELS = 7;

function summary(day: DayUsage): string {
  const requests = `${count(day.requests)} ${day.requests === 1 ? 'request' : 'requests'}`;
  return `${dateLabel(day.date)}: ${money(day.spend_microdollars)}, ${requests}`;
}

export interface SpendDailyChartProps {
  daily: readonly DayUsage[];
}

export function SpendDailyChart({ daily }: SpendDailyChartProps) {
  const titleId = useId();
  const highest = Math.max(0, ...daily.map((day) => day.spend_microdollars));
  if (highest === 0) {
    return <p className="analytics-quiet">{NO_SPEND}</p>;
  }
  const maximum = RunSeriesAxis.ceiling(highest);
  const gridlines = RunSeriesAxis.gridlines(maximum);
  const every = RunSeriesAxis.labelEvery(daily.length, TICK_LABELS);
  const columns = { '--analytics-chart-points': daily.length } as CSSProperties;
  const total = daily.reduce((sum, day) => sum + day.spend_microdollars, 0);

  return (
    <figure className="analytics-chart spend-chart" style={columns}>
      <div className="analytics-chart-plot">
        <div className="analytics-chart-axis" aria-hidden="true">
          {gridlines.map((value) => (
            <span key={value}>{axisMoney(value)}</span>
          ))}
        </div>
        <svg
          className="analytics-chart-canvas"
          viewBox={`0 0 ${daily.length} ${HEIGHT}`}
          preserveAspectRatio="none"
          role="img"
          aria-labelledby={titleId}
        >
          <title id={titleId}>{CHART_CAPTION}</title>
          {gridlines.map((value) => (
            <line
              key={value}
              className="analytics-chart-grid"
              x1={0}
              x2={daily.length}
              y1={HEIGHT - (value / maximum) * HEIGHT}
              y2={HEIGHT - (value / maximum) * HEIGHT}
              vectorEffect="non-scaling-stroke"
            />
          ))}
          {daily.map((day, index) => {
            const height = (day.spend_microdollars / maximum) * HEIGHT;
            return (
              <g key={day.date} className="analytics-chart-bar" data-testid="spend-bar">
                <title>{summary(day)}</title>
                <rect className="analytics-chart-hit" x={index} y={0} width={1} height={HEIGHT} />
                {height > 0 && (
                  <rect
                    className="spend-chart-fill"
                    x={index + BAR_OFFSET}
                    y={HEIGHT - height}
                    width={BAR_WIDTH}
                    height={height}
                    vectorEffect="non-scaling-stroke"
                  />
                )}
              </g>
            );
          })}
        </svg>
      </div>
      <div className="analytics-chart-ticks" aria-hidden="true">
        {daily.map((day, index) => {
          const rank = RunSeriesAxis.rank(index, every);
          return (
            <span key={day.date} data-rank={rank}>
              {rank ? dayLabel(day.date) : ''}
            </span>
          );
        })}
      </div>
      <figcaption className="analytics-legend">
        <span className="analytics-legend-item">
          <span className="analytics-legend-swatch spend-chart-swatch" aria-hidden="true" />
          {SPEND_COLUMN}
          <span className="analytics-legend-value">{money(total)}</span>
        </span>
        <span className="analytics-legend-note">{CHART_NOTE}</span>
      </figcaption>
      <div className="sr-only">
        <table>
          <caption>{CHART_CAPTION}</caption>
          <thead>
            <tr>
              <th scope="col">{DAY_COLUMN}</th>
              <th scope="col">{SPEND_COLUMN}</th>
              <th scope="col">{REQUESTS_COLUMN}</th>
              <th scope="col">{PROMPT_TOKENS_TILE}</th>
              <th scope="col">{COMPLETION_TOKENS_TILE}</th>
            </tr>
          </thead>
          <tbody>
            {daily.map((day) => (
              <tr key={day.date}>
                <th scope="row">{dateLabel(day.date)}</th>
                <td>{money(day.spend_microdollars)}</td>
                <td>{count(day.requests)}</td>
                <td>{count(day.prompt_tokens)}</td>
                <td>{count(day.completion_tokens)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </figure>
  );
}
