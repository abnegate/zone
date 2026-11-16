import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import type { Trend } from '../types';
import { NO_TREND, TrendIndicator } from './TrendIndicator';

const MEASURED: Trend = {
  direction: 'improving',
  earlier_success_rate: 0.68,
  later_success_rate: 0.8,
  change: 0.12,
};

describe('TrendIndicator', () => {
  it('points up by the change in points when improving', () => {
    const { container } = render(<TrendIndicator trend={MEASURED} />);

    expect(screen.getByText('Up 12 points')).toBeDefined();
    const trend = container.querySelector('.analytics-trend');
    expect(trend?.getAttribute('data-direction')).toBe('improving');
    expect(trend?.getAttribute('title')).toBe('Earlier half 68%, later half 80%');
    expect(container.querySelector('[aria-hidden="true"]')?.textContent).toBe('↑');
  });

  it('points down when declining', () => {
    render(<TrendIndicator trend={{ ...MEASURED, direction: 'declining', change: -0.05 }} />);

    expect(screen.getByText('Down 5 points')).toBeDefined();
  });

  it('calls a measured flat window steady', () => {
    render(<TrendIndicator trend={{ ...MEASURED, direction: 'steady', change: 0.02 }} />);

    expect(screen.getByText('Steady')).toBeDefined();
  });

  it('names no trend when either half was too thin to measure', () => {
    render(
      <TrendIndicator
        trend={{
          direction: 'steady',
          earlier_success_rate: null,
          later_success_rate: 0.5,
          change: 0,
        }}
      />
    );

    expect(screen.getByText(NO_TREND)).toBeDefined();
  });
});
